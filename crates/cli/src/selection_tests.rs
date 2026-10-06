//! Top-N Command checks with controlled transports and capacity failures.
use super::*;
use command_test_support::{Input, Output};

fn input(bytes: &[u8], error: Option<i32>) -> Input<'_> {
    Input {
        bytes,
        segment: 1,
        error,
    }
}

fn output() -> Output {
    Output {
        bytes: Vec::new(),
        error: None,
        closed: false,
    }
}

#[test]
fn selection_waits_for_complete_input_and_preserves_short_writes() {
    let bytes = b"A\t2\nB\t3\n";
    for error in [None, Some(5), Some(9)] {
        let mut reader = input(bytes, error);
        let mut writer = output();
        let (status, diagnostics) =
            command_test_support::command(&mut reader, &mut writer, &["top:1", "2"]);
        assert!(writer.closed);
        match error {
            None => {
                assert_eq!((status, diagnostics), (0, vec![]));
                assert_eq!(writer.bytes, b"B\t3\n");
            }
            Some(_) => {
                assert_eq!(status, 1);
                assert_eq!(diagnostics.len(), 1);
                assert!(writer.bytes.is_empty());
            }
        }
    }
}

#[test]
fn csv_selection_ranks_decoded_fields_and_copies_complete_bytes() {
    let mut reader = input(
        b"\"A\",8,first\r\nB,\"10\",\"quote \"\"note\"\"\nline\0\xff\",\r\nC,10,third,\nD,9,fourth",
        None,
    );
    let mut writer = output();
    let result = command_test_support::command(&mut reader, &mut writer, &["--csv", "top:2", "2"]);
    assert_eq!(result, (0, vec![]));
    assert_eq!(
        writer.bytes,
        b"B,10,\"quote \"\"note\"\"\nline\0\xff\",\"\"\nC,10,third,\"\"\n"
    );
    assert!(writer.closed);
}

#[test]
fn csv_selection_observes_decoded_keys_before_missing_rank_omission() {
    let bytes = b"\"key,name\",value,note\nA,2,first\n\"A\",3,second\nB,NA,omitted\nA,1,later\n";
    let mut reader = input(bytes, None);
    let mut writer = output();
    let result = command_test_support::command(
        &mut reader,
        &mut writer,
        &[
            "--csv",
            "-H",
            "-g",
            r"key\,name",
            "--narm",
            "top:1",
            "value",
        ],
    );
    assert_eq!(result, (0, vec![]));
    assert_eq!(
        writer.bytes,
        b"\"key,name\",value,note\nA,3,second\nA,1,later\n"
    );
}

#[test]
fn selection_binds_named_ranking_before_copying_one_header() {
    let mut reader = input(b"id\trea,ding\trea,ding\nA\t2\t9\nB\t3\t1\n", None);
    let mut writer = output();
    let (status, diagnostics) =
        command_test_support::command(&mut reader, &mut writer, &["-H", "top:1", r"rea\,ding"]);
    assert_eq!((status, diagnostics), (0, vec![]));
    assert_eq!(writer.bytes, b"id\trea,ding\trea,ding\nB\t3\t1\n");
    assert!(writer.closed);
}

#[test]
fn selection_observes_missing_only_groups_before_omitting_records() {
    let mut reader = input(
        b"key\tid\tvalue\nA\tfirst\t2\nA\tsecond\t3\nB\tomitted\tNA\nA\tlater\t1\n",
        None,
    );
    let mut writer = output();
    let (status, diagnostics) = command_test_support::command(
        &mut reader,
        &mut writer,
        &["-H", "-g", "key", "--narm", "top:1", "value"],
    );
    assert_eq!((status, diagnostics), (0, vec![]));
    assert_eq!(writer.bytes, b"key\tid\tvalue\nA\tsecond\t3\nA\tlater\t1\n");
}

#[test]
fn sorted_selection_keeps_complete_records_and_original_tie_order() {
    let mut reader = input(
        b"key\tvalue\tnote\nB\t1\tb-first\nA\t10\ta-first\nB\t3\tb-best\nA\t10\ta-second\nA\t10\ta-later\n",
        None,
    );
    let mut writer = output();
    let (status, diagnostics) = command_test_support::command(
        &mut reader,
        &mut writer,
        &["-s", "-H", "-g", "key", "top:2", "value"],
    );
    assert_eq!((status, diagnostics), (0, vec![]));
    assert_eq!(
        writer.bytes,
        b"key\tvalue\tnote\nA\t10\ta-first\nA\t10\ta-second\nB\t3\tb-best\nB\t1\tb-first\n"
    );
    assert!(writer.closed);
}

#[test]
fn unavailable_controls_and_malformed_requests_leave_input_untouched() {
    for (args, status) in [
        (vec!["-s", "-g", "1", "--round", "2", "top:1", "1"], 77),
        (vec!["--sort-cmd", "sort", "TOP:0", "1"], 1),
        (vec!["--sort-cmd", "sort", r"t\op:0", "1"], 1),
        (vec!["--sort-cmd", "sort", "gb", "1", "TOP:0", "2"], 1),
        (vec!["--round", "2", "top:1", "1"], 77),
        (vec!["top:1", "named"], 1),
        (vec!["--csv", "top:1", "named"], 1),
        (vec!["--csv", "-z", "top:1", "1"], 1),
        (vec!["top:0", "1"], 1),
    ] {
        let mut reader = input(b"do not consume", Some(5));
        let mut writer = output();
        let (actual, diagnostics) = command_test_support::command(&mut reader, &mut writer, &args);
        assert_eq!(actual, status, "{args:?}: {diagnostics:?}");
        assert!(!diagnostics.is_empty());
        assert!(writer.bytes.is_empty());
        assert_eq!(reader.bytes, b"do not consume");
    }
}

#[test]
fn selection_uses_the_effective_supported_numeric_locale() {
    let environment = Environment {
        locale: locale::Policy::resolve(|key| (key == "LC_ALL").then(|| "de_DE.UTF-8".into())),
        posixly_correct: false,
        grouping: None,
        sort_memory: None,
        pipe_grouping: None,
        terminal: Default::default(),
    };
    let mut reader = input(b"A;1,25\nB;1,5\nC;-2,75\n", None);
    let mut writer = output();
    let result = command_test_support::command_in(
        &mut reader,
        &mut writer,
        &["-t", ";", "top:2", "2"],
        environment,
    );
    assert_eq!(result, (0, vec![]));
    assert_eq!(writer.bytes, b"B;1,5\nA;1,25\n");
}

#[test]
fn selection_keeps_completed_groups_but_not_the_group_ended_by_failure() {
    let bytes = b"# preamble\nkey\tvalue\nA\t2\nA\t3\n; comment\nB\t4\nB\t1\n";
    for error in [Some(5), Some(9)] {
        let mut reader = input(bytes, error);
        let mut writer = output();
        let (status, diagnostics) = command_test_support::command(
            &mut reader,
            &mut writer,
            &["-C", "-H", "-g", "key", "top:1", "value"],
        );
        assert_eq!(status, 1);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(writer.bytes, b"key\tvalue\nA\t3\n");
        assert!(writer.closed);
    }
    let invalid = [bytes.as_slice(), b"B\tbad\n"].concat();
    let mut reader = input(&invalid, None);
    let mut writer = output();
    let (status, diagnostics) = command_test_support::command(
        &mut reader,
        &mut writer,
        &["-C", "-H", "-g", "key", "top:1", "value"],
    );
    assert_eq!(status, 1);
    assert_eq!(
        diagnostics,
        [b"invalid numeric value in line 6 field 2: 'bad'\n".to_vec()]
    );
    assert_eq!(writer.bytes, b"key\tvalue\nA\t3\n");
    assert!(writer.closed);
}

#[test]
fn selection_checks_later_keys_before_missing_rank_omission_or_group_transition() {
    let mut reader = input(
        b"# ignored\nkey\tvalue\textra\nA\t2\tx\n; ignored\nB\tNA\n",
        None,
    );
    let mut writer = output();
    let (status, diagnostics) = command_test_support::command(
        &mut reader,
        &mut writer,
        &["-C", "-H", "-g", "1,3", "--narm", "top:1", "value"],
    );
    assert_eq!(status, 1);
    assert_eq!(
        diagnostics,
        [b"invalid input: field 3 requested, line 3 has only 2 fields\n".to_vec()]
    );
    assert_eq!(writer.bytes, b"key\tvalue\textra\n");
    assert!(writer.closed);
}

#[test]
fn grouped_case_controls_are_admitted_before_input_but_no_key_case_is_inert() {
    for (grouped, sort) in [(false, false), (false, true), (true, false), (true, true)] {
        let environment = Environment {
            locale: locale::Policy::resolve(|key| {
                (key == "LC_CTYPE").then(|| "tr_TR.UTF-8".into())
            }),
            posixly_correct: false,
            grouping: None,
            sort_memory: None,
            pipe_grouping: None,
            terminal: Default::default(),
        };
        let bytes = b"key\tvalue\ni\t2\nI\t3\n";
        let mut reader = input(bytes, None);
        let mut writer = output();
        let mut args = vec!["-H", "-i"];
        if sort {
            args.push("-s");
        }
        if grouped {
            args.extend(["-g", "key"]);
        }
        args.extend(["top:1", "value"]);
        let (status, diagnostics) =
            command_test_support::command_in(&mut reader, &mut writer, &args, environment);
        if grouped {
            assert_eq!(status, 77);
            assert!(diagnostics[0].starts_with(b"unsupported -i under character type "));
            assert_eq!(reader.bytes, bytes);
            assert!(writer.bytes.is_empty());
        } else {
            assert_eq!((status, diagnostics), (0, vec![]));
            assert_eq!(writer.bytes, b"key\tvalue\nI\t3\n");
        }
    }
}

struct ClosingOutput {
    inner: Output,
    close_error: Option<i32>,
    zero_write: bool,
}
impl Write for ClosingOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.zero_write {
            Ok(0)
        } else {
            self.inner.write(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl command_output::Transport for ClosingOutput {
    fn buffering(&self) -> (usize, bool) {
        (1, false)
    }
    fn close(&mut self) -> io::Result<()> {
        self.inner.closed = true;
        self.close_error
            .map_or(Ok(()), |code| Err(io::Error::from_raw_os_error(code)))
    }
}

#[test]
fn selection_reports_write_and_close_failures_using_transport_precedence() {
    for (args, bytes) in [
        (vec!["top:1", "2"], b"A\t2\n".as_slice()),
        (
            vec!["-H", "-g", "key", "top:1", "value"],
            b"key\tvalue\nA\t2\nB\t3\n",
        ),
        (
            vec!["-s", "-H", "-g", "key", "top:1", "value"],
            b"key\tvalue\nB\t3\nA\t2\n",
        ),
        (vec!["--csv", "top:1", "2"], b"A,2\n"),
        (
            vec!["--csv-in", "-H", "-g", "key", "top:1", "value"],
            b"key,value\nA,2\nB,3\n",
        ),
        (
            vec!["--csv", "-H", "-g", "key", "top:1", "value"],
            b"key,value\nA,2\nB,3\n",
        ),
        (
            vec!["--csv-out", "-s", "-H", "-g", "key", "top:1", "value"],
            b"key\tvalue\nB\t3\nA\t2\n",
        ),
        (
            vec!["--csv", "-s", "-H", "-g", "key", "top:1", "value"],
            b"key,value\nB,3\nA,2\n",
        ),
        (
            vec!["--csv-in", "-s", "-H", "-g", "key", "top:1", "value"],
            b"key,value\nB,3\nA,2\n",
        ),
    ] {
        for (write, close, zero, expected) in [
            (Some(28), None, false, 1),
            (Some(5), None, false, 77),
            (None, Some(28), false, 1),
            (None, Some(5), false, 77),
            (None, None, true, 77),
            (Some(28), Some(5), false, 77),
        ] {
            let mut reader = input(bytes, None);
            let mut writer = ClosingOutput {
                inner: output(),
                close_error: close,
                zero_write: zero,
            };
            writer.inner.error = write;
            let (status, diagnostics) =
                command_test_support::command(&mut reader, &mut writer, &args);
            assert_eq!(
                status, expected,
                "write={write:?}, close={close:?}: {diagnostics:?}"
            );
            assert!(!diagnostics.is_empty());
            assert!(writer.inner.closed);
        }
    }
    let mut reader = input(b"bad\n", None);
    let mut writer = ClosingOutput {
        inner: output(),
        close_error: Some(5),
        zero_write: false,
    };
    let (status, diagnostics) =
        command_test_support::command(&mut reader, &mut writer, &["top:1", "1"]);
    assert_eq!(status, 77);
    assert_eq!(diagnostics.len(), 2);
    assert!(writer.inner.bytes.is_empty());

    let mut reader = input(b"key\tvalue\nA\t2\nB\tbad\n", None);
    let mut writer = ClosingOutput {
        inner: output(),
        close_error: None,
        zero_write: false,
    };
    writer.inner.error = Some(28);
    let (status, diagnostics) = command_test_support::command(
        &mut reader,
        &mut writer,
        &["-H", "-g", "key", "top:1", "value"],
    );
    assert_eq!(status, 1);
    assert_eq!(
        diagnostics[0],
        b"invalid numeric value in line 3 field 2: 'bad'\n"
    );
    assert_eq!(diagnostics.len(), 2);
    assert!(writer.inner.closed);
}

fn sorted_environment(memory: Option<&str>) -> Environment {
    Environment {
        locale: Default::default(),
        posixly_correct: false,
        grouping: None,
        sort_memory: memory.map(Into::into),
        pipe_grouping: None,
        terminal: Default::default(),
    }
}

#[test]
fn csv_sorted_source_observation_and_spill_keep_segmented_records() {
    let bytes =
        b"key,value,note\r\nb,1,first\na,3,\"two\r\nlines\"\nb,2,last\na,3,\"a\"\"later\"\n";
    for segment in 1..=bytes.len() {
        for memory in ["67108864", "1"] {
            let mut reader = input(bytes, None);
            reader.segment = segment;
            let mut writer = output();
            assert_eq!(
                command_test_support::command_in(
                    &mut reader,
                    &mut writer,
                    &["--csv", "-H", "-s", "-g", "key", "top:2", "value"],
                    sorted_environment(Some(memory)),
                ),
                (0, vec![]),
            );
            assert_eq!(
                writer.bytes,
                b"key,value,note\na,3,\"two\r\nlines\"\na,3,\"a\"\"later\"\nb,2,last\nb,1,first\n"
            );
            assert!(writer.closed);
        }
    }
}

#[test]
fn csv_sorted_read_failure_never_flushes_an_incomplete_group() {
    for memory in ["67108864", "1"] {
        for csv_out in [false, true] {
            for output_error in [None, Some(5)] {
                let mut reader = input(b"key,value\nb,1\na,3\nb,2\n", Some(5));
                let mut writer = output();
                writer.error = output_error;
                let result = command_test_support::command_in(
                    &mut reader,
                    &mut writer,
                    &[
                        if csv_out { "--csv" } else { "--csv-in" },
                        "-H",
                        "-s",
                        "-g",
                        "key",
                        "top:1",
                        "value",
                    ],
                    sorted_environment(Some(memory)),
                );
                assert_eq!(result.0, if output_error.is_some() { 77 } else { 1 });
                assert_eq!(result.1[0], b"read error: Input/output error\n");
                if output_error.is_some() {
                    assert_eq!(result.1[1], b"unsupported output I/O error\n");
                    assert!(writer.bytes.is_empty());
                } else {
                    assert_eq!(
                        writer.bytes,
                        if csv_out {
                            b"key,value\n".as_slice()
                        } else {
                            b"key\tvalue\n"
                        }
                    );
                }
                assert!(writer.closed);
            }
        }
    }
}

#[test]
fn csv_sorted_spill_faults_and_capacity_keep_completion_precedence() {
    use projected_sort::{RunFault, fail_run};
    let bytes = [
        b"key,value\n".as_slice(),
        b"b,1\na,2\n".repeat(12).as_slice(),
    ]
    .concat();
    for (fault, at) in [
        (RunFault::Write, 0),
        (RunFault::Write, 8),
        (RunFault::Write, 24),
        (RunFault::Read, 0),
        (RunFault::Read, 30),
        (RunFault::Merge, 0),
        (RunFault::Merge, 2),
    ] {
        for output_error in [None, Some(5)] {
            let mut reader = input(&bytes, None);
            let mut writer = output();
            writer.error = output_error;
            fail_run(Some((fault, at, 5)));
            let result = command_test_support::command_in(
                &mut reader,
                &mut writer,
                &["--csv", "-H", "-s", "-g", "key", "top:2", "value"],
                sorted_environment(Some("1")),
            );
            fail_run(None);
            assert_eq!(
                result.0,
                if output_error.is_some() { 77 } else { 1 },
                "{result:?}"
            );
            assert_eq!(
                result.1[0],
                b"sort temporary I/O error: Input/output error (os error 5)\n"
            );
            if output_error.is_some() {
                assert_eq!(result.1[1], b"unsupported output I/O error\n");
            }
            assert!(b"key,value\na,2\na,2\nb,1\nb,1\n".starts_with(&writer.bytes));
            assert!(writer.closed);
        }
    }
    for memory in ["67108864", "1"] {
        let mut completed = false;
        for at in 0..600 {
            let mut reader = input(&bytes, None);
            let mut writer = output();
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = command_test_support::command_in(
                &mut reader,
                &mut writer,
                &["--csv", "-H", "-s", "-g", "key", "top:2", "value"],
                sorted_environment(Some(memory)),
            );
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            assert!(writer.closed || writer.bytes.is_empty());
            let expected = b"key,value\na,2\na,2\nb,1\nb,1\n";
            if result.0 == 0 {
                assert!(result.1.is_empty());
                assert_eq!(writer.bytes, expected);
                completed = true;
                break;
            }
            assert_eq!(result.0, 77, "{at}: {result:?}");
            assert!(
                result
                    .1
                    .iter()
                    .any(|message| message.ends_with(b"memory allocation failed\n")),
                "{result:?}"
            );
            assert!(expected.starts_with(&writer.bytes));
        }
        assert!(completed, "memory={memory}");
    }
}

#[test]
fn sorted_selection_admits_locale_and_resource_controls_before_input() {
    let bytes = b"key\tvalue\nB\t3\nA\t2\n";
    for (memory, collate, message) in [
        (Some("bad"), "C", b"FASTMASH_SORT_MEMORY_BYTES".as_slice()),
        (None, "cs_CZ.UTF-8", b"unsupported sorting".as_slice()),
    ] {
        let mut environment = sorted_environment(memory);
        environment.locale = locale::Policy::resolve(|key| match key {
            "LC_COLLATE" => Some(collate.into()),
            "LC_NUMERIC" | "LC_CTYPE" => Some("C".into()),
            _ => None,
        });
        let mut reader = input(bytes, Some(5));
        let mut writer = output();
        let (status, diagnostics) = command_test_support::command_in(
            &mut reader,
            &mut writer,
            &["-s", "-H", "-g", "key", "top:1", "value"],
            environment,
        );
        assert_eq!(status, 77, "{diagnostics:?}");
        assert!(diagnostics[0].starts_with(message));
        assert_eq!(reader.bytes, bytes);
        assert!(writer.bytes.is_empty());
    }
    let mut reader = input(bytes, None);
    let mut writer = output();
    assert_eq!(
        command_test_support::command_in(
            &mut reader,
            &mut writer,
            &["-s", "-H", "top:1", "value"],
            sorted_environment(Some("bad")),
        ),
        (0, vec![])
    );
    assert_eq!(writer.bytes, b"key\tvalue\nB\t3\n");
}

#[test]
fn sorted_selection_read_failures_emit_no_incomplete_sorted_groups() {
    for memory in ["67108864", "1"] {
        for error in [5, 9] {
            let mut reader = input(b"key\tvalue\nB\t3\nA\t2\nB\t4\n", Some(error));
            let mut writer = output();
            let (status, diagnostics) = command_test_support::command_in(
                &mut reader,
                &mut writer,
                &["-s", "-H", "-g", "key", "top:1", "value"],
                sorted_environment(Some(memory)),
            );
            assert_eq!(status, 1);
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(writer.bytes, b"key\tvalue\n");
            assert!(writer.closed);
        }
    }
}

#[test]
fn sorted_selection_allocation_refusals_preserve_status_and_finalization() {
    let mut refused = 0;
    let mut completed = 0;
    for memory in ["67108864", "1"] {
        for after in 0..30 {
            command_memory::FAIL_RESERVATION.with(|remaining| remaining.set(Some(after)));
            let mut reader = input(b"B\t3\nA\t2\nB\t4\nA\t1\n", None);
            let mut writer = output();
            let (status, diagnostics) = command_test_support::command_in(
                &mut reader,
                &mut writer,
                &["-s", "-g", "1", "top:1", "2"],
                sorted_environment(Some(memory)),
            );
            command_memory::FAIL_RESERVATION.with(|remaining| remaining.set(None));
            if status == 77 {
                refused += 1;
                assert_eq!(
                    diagnostics,
                    [b"command memory allocation failed\n".to_vec()]
                );
            } else {
                completed += 1;
                assert_eq!((status, diagnostics), (0, vec![]));
                assert_eq!(writer.bytes, b"A\t2\nB\t4\n");
            }
            // Startup refusals occur before the output transport is acquired.
            if reader.bytes.is_empty() {
                assert!(writer.closed);
            }
        }
    }
    assert!(refused > 0 && completed > 0);
}

/// Generates each suffix Record on demand rather than retaining the input.
struct RepeatedInput {
    csv: bool,
    first: bool,
    remaining: usize,
    position: usize,
}
impl io::Read for RepeatedInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for RepeatedInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(if self.first && self.csv {
            &b"winner,9\n"[self.position..]
        } else if self.first {
            &b"winner\t9\n"[self.position..]
        } else if self.remaining != 0 && self.csv {
            &b"other,1\n"[self.position..]
        } else if self.remaining != 0 {
            &b"other\t1\n"[self.position..]
        } else {
            b""
        })
    }
    fn consume(&mut self, count: usize) {
        self.position += count;
        let length = if self.first { 9 } else { 8 };
        if self.position == length {
            self.position = 0;
            if self.first {
                self.first = false;
            } else {
                self.remaining -= 1;
            }
        }
    }
}
impl replay::Rewind for RepeatedInput {}

#[test]
fn capacity_faults_refuse_safely_and_nonwinning_suffixes_keep_bounded_retention() {
    let mut observed_refusal = false;
    let mut observed_success = false;
    for (csv, at) in [false, true]
        .into_iter()
        .flat_map(|csv| (0..64).map(move |at| (csv, at)))
    {
        let mut results = Vec::new();
        for remaining in [2, 100_000] {
            let mut reader = RepeatedInput {
                csv,
                first: true,
                remaining,
                position: 0,
            };
            let mut writer = output();
            let args: Vec<std::ffi::OsString> = if csv {
                vec!["--csv-in".into(), "top:1".into(), "2".into()]
            } else {
                vec!["top:1".into(), "2".into()]
            };
            let environment = Environment {
                locale: Default::default(),
                posixly_correct: false,
                grouping: None,
                sort_memory: None,
                pipe_grouping: None,
                terminal: Default::default(),
            };
            let mut diagnostics = Vec::new();
            let mut report = |failure: &Failure| {
                diagnostics.push(failure.message.clone());
                true
            };
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = run_in(
                &mut reader,
                &mut writer,
                &args,
                b"fastmash",
                &mut report,
                environment,
            );
            let status = result.unwrap_or_else(|failure| {
                report(&failure);
                failure.status
            });
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            if status == 0 {
                observed_success = true;
                assert_eq!(writer.bytes, b"winner\t9\n");
                assert!(diagnostics.is_empty());
            } else {
                observed_refusal = true;
                assert_eq!(status, 77);
                assert!(writer.bytes.is_empty());
                assert_eq!(
                    diagnostics,
                    [b"command memory allocation failed\n".to_vec()]
                );
            }
            results.push((status, diagnostics, writer.bytes));
        }
        assert_eq!(results[0], results[1], "allocation fault schedule {at}");
    }
    assert!(observed_refusal && observed_success);
}

#[test]
fn csv_selection_checks_all_syntax_at_chunk_boundaries_before_omission() {
    for segment in [1, 2, 3, 127, 128 * 1024] {
        for rank in ["1", "NA"] {
            let bytes = format!("A,inf,first\nB,{rank},\"unused\nfield\"x\n");
            let mut reader = Input {
                bytes: bytes.as_bytes(),
                segment,
                error: None,
            };
            let mut writer = output();
            let (status, diagnostics) = command_test_support::command(
                &mut reader,
                &mut writer,
                &["--csv", "--narm", "top:1", "2"],
            );
            assert_eq!(status, 1);
            assert_eq!(diagnostics, [b"invalid CSV: byte after closing quote at physical line 3 byte 7; record 2 starts at physical line 2\n".to_vec()]);
            assert!(writer.bytes.is_empty());
            assert!(writer.closed);
        }
    }
}

#[test]
fn csv_selection_keeps_only_completed_groups_after_input_failure() {
    let bytes = b"key,value,note\nA,2,first\nA,3,second\nB,4,\"two\nlines\"\nB,1,last\n";
    for csv_out in [false, true] {
        for grouped in [false, true] {
            for error in [Some(5), Some(9)] {
                let mut reader = input(bytes, error);
                let mut writer = output();
                let mut args = vec![if csv_out { "--csv" } else { "--csv-in" }, "-H"];
                if grouped {
                    args.extend(["-g", "key"]);
                }
                args.extend(["top:1", "value"]);
                let (status, diagnostics) =
                    command_test_support::command(&mut reader, &mut writer, &args);
                assert_eq!(status, 1);
                assert_eq!(diagnostics.len(), 1);
                let expected = match (csv_out, grouped) {
                    (false, false) => b"key\tvalue\tnote\n".as_slice(),
                    (false, true) => b"key\tvalue\tnote\nA\t3\tsecond\n",
                    (true, false) => b"key,value,note\n",
                    (true, true) => b"key,value,note\nA,3,second\n",
                };
                assert_eq!(writer.bytes, expected);
                assert!(writer.closed);
            }
        }
    }
    for suffix in [b"B,bad,late\n".as_slice(), b"B,NA,\"unfinished"] {
        let invalid = [bytes.as_slice(), suffix].concat();
        let mut reader = input(&invalid, None);
        let mut writer = output();
        let (status, diagnostics) = command_test_support::command(
            &mut reader,
            &mut writer,
            &["--csv", "-H", "-g", "key", "--narm", "top:1", "value"],
        );
        assert_eq!(status, 1);
        assert_eq!(writer.bytes, b"key,value,note\nA,3,second\n");
        assert_eq!(diagnostics.len(), 1);
        let location = b"record 6 starts at physical line 7";
        assert!(
            diagnostics[0]
                .windows(location.len())
                .any(|part| part == location)
        );
    }
}

#[test]
fn csv_selection_uses_field_local_conversion_and_admitted_locale() {
    let mut reader = input(b"A,1,e99999\nB,2,ignored\n", None);
    let mut writer = output();
    assert_eq!(
        command_test_support::command(&mut reader, &mut writer, &["--csv", "bottom:1", "2"]),
        (0, vec![])
    );
    assert_eq!(writer.bytes, b"A,1,e99999\n");
    let environment = Environment {
        locale: locale::Policy::resolve(|key| (key == "LC_ALL").then(|| "de_DE.UTF-8".into())),
        posixly_correct: false,
        grouping: None,
        sort_memory: None,
        pipe_grouping: None,
        terminal: Default::default(),
    };
    let mut reader = input(b"A,\"1,25\"\nB,\"1,5\"\nC,\"-2,75\"\n", None);
    let mut writer = output();
    assert_eq!(
        command_test_support::command_in(
            &mut reader,
            &mut writer,
            &["--csv", "top:2", "2"],
            environment
        ),
        (0, vec![])
    );
    assert_eq!(writer.bytes, b"B,\"1,5\"\nA,\"1,25\"\n");
}
