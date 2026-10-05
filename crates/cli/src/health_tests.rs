//! Health command checks with controlled transport and capacity failures.
use super::*;

struct Input<'a> {
    bytes: &'a [u8],
    segment: usize,
    error: Option<i32>,
}
impl io::Read for Input<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = bytes.len().min(available.len());
        bytes[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for Input<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.bytes.is_empty()
            && let Some(code) = self.error
        {
            return Err(io::Error::from_raw_os_error(code));
        }
        Ok(&self.bytes[..self.bytes.len().min(self.segment)])
    }
    fn consume(&mut self, count: usize) {
        self.bytes = &self.bytes[count..];
    }
}
impl replay::Rewind for Input<'_> {}

#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    write_error: Option<i32>,
    write_error_after: usize,
    close_error: Option<i32>,
    closed: bool,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(code) = self.write_error
            && self.bytes.len() >= self.write_error_after
        {
            return Err(io::Error::from_raw_os_error(code));
        }
        let mut count = bytes.len().min(2);
        if self.write_error.is_some() {
            count = count.min(self.write_error_after - self.bytes.len());
        }
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl command_output::Transport for Output {
    fn buffering(&self) -> (usize, bool) {
        (1, false)
    }
    fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        self.close_error
            .map_or(Ok(()), |code| Err(io::Error::from_raw_os_error(code)))
    }
}

fn command(input: &mut Input<'_>, output: &mut Output, args: &[&str]) -> (i32, Vec<Vec<u8>>) {
    let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
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
    let status = run_in(input, output, &args, b"fastmash", &mut report, environment)
        .unwrap_or_else(|failure| {
            report(&failure);
            failure.status
        });
    (status, diagnostics)
}

#[test]
fn health_tsv_transport_completion_and_fault_statuses_match_readable() {
    for format in [None, Some("tsv")] {
        let mut arguments = vec![
            "-H",
            "health",
            "type",
            "2",
            "integer",
            "nonmissing",
            "2",
            "validate",
        ];
        arguments.extend(format);
        let mut expected = None;
        for segment in [1, 7, 128] {
            for (write_error, close_error, expected_status) in [
                (None, None, 1),
                (Some(9), None, 1),
                (Some(28), None, 1),
                (Some(5), None, 77),
                (None, Some(28), 1),
                (None, Some(5), 77),
            ] {
                let mut input = Input {
                    bytes: b"id\tvalue\n1\t1.5\n2\tNA\n3\n",
                    segment,
                    error: None,
                };
                let mut output = Output {
                    write_error,
                    close_error,
                    ..Default::default()
                };
                let (status, diagnostics) = command(&mut input, &mut output, &arguments);
                assert_eq!(status, expected_status);
                assert_eq!(
                    diagnostics.len(),
                    usize::from(write_error.is_some() || close_error.is_some())
                );
                assert!(output.closed && input.bytes.is_empty());
                if diagnostics.is_empty() {
                    if let Some(expected) = &expected {
                        assert_eq!(&output.bytes, expected);
                    } else {
                        expected = Some(output.bytes);
                    }
                }
            }
        }
        for bytes in [
            b"".as_slice(),
            b"id\tvalue\n1\t1.5\n",
            b"id\tvalue\n1\tNA\npartial",
        ] {
            let mut input = Input {
                bytes,
                segment: 1,
                error: Some(5),
            };
            let mut output = Output::default();
            assert_eq!(
                command(&mut input, &mut output, &arguments),
                (1, vec![b"read error: Input/output error\n".to_vec()])
            );
            assert!(output.closed && output.bytes.is_empty());
        }
    }
    let mut input = Input {
        bytes: b"a\nb\n",
        segment: 1,
        error: None,
    };
    let mut output = Output::default();
    health::COUNT_LIMIT.with(|limit| limit.set(Some(1)));
    let result = command(&mut input, &mut output, &["health", "tsv"]);
    health::COUNT_LIMIT.with(|limit| limit.set(None));
    assert_eq!(
        result,
        (77, vec![b"health count exceeds u64 limit\n".to_vec()])
    );
    assert!(output.closed && output.bytes.is_empty());
}

#[test]
fn health_tsv_checked_allocations_prevent_partial_reports() {
    for limit in ["0", "2"] {
        let mut refusals = 0;
        let mut completed = false;
        for at in 0..500 {
            let mut input = Input {
                bytes: b"a\tb\tb\t\n1\t2\tNA\t\n3\t4.5\tx\t4\n5\tbad\tN/A\t6\n\n7\t8\tNaN\toops\textra\n",
                segment: 1,
                error: None,
            };
            let mut output = Output::default();
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = command(
                &mut input,
                &mut output,
                &[
                    "-H",
                    "health",
                    "tsv",
                    "width",
                    "3",
                    "type",
                    "2",
                    "integer",
                    "type",
                    "3",
                    "text",
                    "required",
                    "1",
                    "nonmissing",
                    "3",
                    "examples",
                    limit,
                    "validate",
                ],
            );
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            if result == (1, Vec::new()) {
                let report = String::from_utf8(output.bytes).unwrap();
                assert!(report.starts_with("row_kind\tcategory\tbasis\t"));
                assert!(
                    report.contains("summary\tdata_records\tobservation\t\t\t5\tdata_records\t")
                );
                assert!(report.contains("finding\tmixed_types\tobservation\t2\tb\t1\tfields\t"));
                assert!(
                    report.contains("finding\ttype_mismatch\tinferred\t4\t\t1\tcells\t\tinteger\t")
                );
                assert_eq!(
                    report.lines().any(|row| row.starts_with("example\t")),
                    limit != "0"
                );
                assert!(output.closed && input.bytes.is_empty());
                completed = true;
                break;
            }
            assert_eq!(
                result,
                (77, vec![b"command memory allocation failed\n".to_vec()]),
                "allocation {at}"
            );
            assert!(output.bytes.is_empty());
            refusals += 1;
        }
        assert!(completed && refusals > 0);
    }
}

#[test]
fn health_retains_complete_reports_across_segmentation_and_short_writes() {
    let mut expected = None;
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes: b"x\t\tx\na\nb\tNA\t\n",
            segment,
            error: None,
        };
        let mut output = Output::default();
        assert_eq!(
            command(&mut input, &mut output, &["-H", "health"]),
            (0, Vec::new())
        );
        assert!(output.closed);
        if let Some(expected) = &expected {
            assert_eq!(&output.bytes, expected);
        } else {
            expected = Some(output.bytes);
        }
    }
}

#[test]
fn health_styling_preserves_short_writes_and_partial_failure_completion() {
    for mode in ["--color=never", "--color=always"] {
        for format in [None, Some("tsv")] {
            for (bytes, validation_status) in [(b"1\n2\n".as_slice(), 0), (b"1\n2\nword\nNA\n", 1)]
            {
                let mut args = vec![mode, "health", "type", "1", "integer", "validate"];
                args.extend(format);
                let mut input = Input {
                    bytes,
                    segment: 1,
                    error: None,
                };
                let mut complete = Output::default();
                assert_eq!(
                    command(&mut input, &mut complete, &args),
                    (validation_status, Vec::new())
                );
                assert!(complete.closed && input.bytes.is_empty());
                assert_eq!(
                    complete.bytes.contains(&0x1b),
                    mode == "--color=always" && format.is_none()
                );
                for segment in [1, 7, 128] {
                    for (code, expected_status) in [(9, 1), (28, 1), (5, 77)] {
                        for after in [0, 2, 20, complete.bytes.len() / 2] {
                            let mut input = Input {
                                bytes,
                                segment,
                                error: None,
                            };
                            let mut output = Output {
                                write_error: Some(code),
                                write_error_after: after,
                                ..Default::default()
                            };
                            let (status, diagnostics) = command(&mut input, &mut output, &args);
                            assert_eq!(status, expected_status);
                            assert_eq!(diagnostics.len(), 1);
                            assert_eq!(output.bytes, complete.bytes[..after]);
                            assert!(output.closed && input.bytes.is_empty());
                        }
                    }
                    for (close_error, expected_status) in
                        [(None, validation_status), (Some(28), 1), (Some(5), 77)]
                    {
                        let mut input = Input {
                            bytes,
                            segment,
                            error: None,
                        };
                        let mut output = Output {
                            close_error,
                            ..Default::default()
                        };
                        let (status, diagnostics) = command(&mut input, &mut output, &args);
                        assert_eq!(status, expected_status);
                        assert_eq!(diagnostics.len(), usize::from(close_error.is_some()));
                        assert_eq!(output.bytes, complete.bytes);
                        assert!(output.closed && input.bytes.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn health_read_failures_emit_no_completed_or_partial_report() {
    for bytes in [
        b"".as_slice(),
        b"a\tb\nshort",
        b"a\tb\nshort\n",
        b"NA\t\nwide\tNaN\tlate\n",
    ] {
        let mut input = Input {
            bytes,
            segment: 1,
            error: Some(5),
        };
        let mut output = Output::default();
        assert_eq!(
            command(&mut input, &mut output, &["health"]),
            (1, vec![b"read error: Input/output error\n".to_vec()])
        );
        assert!(output.bytes.is_empty());
        assert!(output.closed);
    }
}

#[test]
fn health_writing_or_finalizing_a_report_cannot_return_success() {
    for (code, status) in [(9, 1), (28, 1), (5, 77)] {
        for fail_close in [false, true] {
            // The shared completion policy ignores a close-only EBADF once
            // successful raw writes have left no pending bytes.
            if code == 9 && fail_close {
                continue;
            }
            let mut input = Input {
                bytes: b"a\tNA\n\n",
                segment: 1,
                error: None,
            };
            let mut output = if fail_close {
                Output {
                    close_error: Some(code),
                    ..Default::default()
                }
            } else {
                Output {
                    write_error: Some(code),
                    ..Default::default()
                }
            };
            let (actual, diagnostics) = command(&mut input, &mut output, &["health"]);
            assert_eq!(actual, status);
            assert_eq!(diagnostics.len(), 1);
            assert!(output.closed);
            assert!(input.bytes.is_empty());
        }
    }
}

#[test]
fn health_allocations_refuse_without_emitting_partial_report() {
    for (bytes, records, field4) in [
        (
            b"a\t\ta\n1\n\n2\t\tNA\n4\tNaN\t6\t\n".as_slice(),
            4,
            "absent_values 3; empty_values 1; missing_values 0",
        ),
        (
            b"a\t\ta\n1\t1.5\tword\n2\t2.5\t9\nlate\ttext\tmore\n3\t3.5\tlast\tNA\n4\n".as_slice(),
            5,
            "absent_values 4; empty_values 0; missing_values 1",
        ),
    ] {
        for limit in ["0", "2"] {
            let mut refusals = 0;
            let mut completed = false;
            for at in 0..250 {
                let mut input = Input {
                    bytes,
                    segment: 1,
                    error: None,
                };
                let mut output = Output::default();
                command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
                let (status, diagnostics) = command(
                    &mut input,
                    &mut output,
                    &["-H", "health", "examples", limit],
                );
                command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
                if status == 0 {
                    assert!(diagnostics.is_empty());
                    assert!(output.bytes.starts_with(
                        format!("Table health report\nData records: {records}\n").as_bytes()
                    ));
                    let output = String::from_utf8(output.bytes).unwrap();
                    assert!(output.contains(&format!(
                    "field 4, name \"\": {field4}; integer_values 0; other_number_values 0; text_values 0\n"
                )));
                    assert!(output.contains("missing_indicator"));
                    if records == 5 {
                        assert!(output.contains("mixed_types"));
                        assert!(output.contains("type_mismatch [inferred, advisory]"));
                    }
                    assert_eq!(output.contains("    accepted record "), limit != "0");
                    completed = true;
                    break;
                }
                assert_eq!(status, 77);
                assert_eq!(
                    diagnostics,
                    vec![b"command memory allocation failed\n".to_vec()]
                );
                assert!(output.bytes.is_empty());
                refusals += 1;
            }
            assert!(completed && refusals > 0);
        }
    }
}

#[test]
fn health_counter_capacity_failure_emits_no_report() {
    let mut input = Input {
        bytes: b"a\nb\n",
        segment: 1,
        error: None,
    };
    let mut output = Output::default();
    health::COUNT_LIMIT.with(|limit| limit.set(Some(1)));
    let result = command(&mut input, &mut output, &["health"]);
    health::COUNT_LIMIT.with(|limit| limit.set(None));
    assert_eq!(
        result,
        (77, vec![b"health count exceeds u64 limit\n".to_vec()])
    );
    assert!(output.bytes.is_empty());
    assert!(output.closed);
}

#[test]
fn health_declaration_binding_errors_precede_data_reads_and_report_emission() {
    for (header, rules, expected) in [
        (
            b"a\ta\n".as_slice(),
            vec!["required", "a"],
            b"health: field name 'a' is ambiguous in Input header\n".as_slice(),
        ),
        (
            b"a\tb\n",
            vec!["type", "2", "integer", "type", "b", "number"],
            b"health: conflicting types for field 2\n",
        ),
        (
            b"a\0tail\n",
            vec!["required", "a"],
            b"health: field name 'a' not found in Input header\n",
        ),
    ] {
        let mut input = Input {
            bytes: header,
            segment: 1,
            error: Some(5),
        };
        let mut output = Output::default();
        let mut args = vec!["-H", "health"];
        args.extend(rules);
        assert_eq!(
            command(&mut input, &mut output, &args),
            (1, vec![expected.to_vec()])
        );
        assert!(output.bytes.is_empty());
        assert!(output.closed);
    }
    let mut input = Input {
        bytes: b"",
        segment: 1,
        error: Some(5),
    };
    let mut output = Output::default();
    assert_eq!(
        command(&mut input, &mut output, &["-H", "health", "required", "a"]),
        (1, vec![b"read error: Input/output error\n".to_vec()])
    );
}

#[test]
fn declared_health_read_failures_never_emit_a_partial_report() {
    for bytes in [
        b"".as_slice(),
        b"a\tb\n1\tNA\n",
        b"a\tb\n1\t1.5\n2\npartial",
    ] {
        let mut input = Input {
            bytes,
            segment: 1,
            error: Some(5),
        };
        let mut output = Output::default();
        assert_eq!(
            command(
                &mut input,
                &mut output,
                &[
                    "-H",
                    "health",
                    "width",
                    "0",
                    "type",
                    "2",
                    "integer",
                    "nonmissing",
                    "5",
                    "validate"
                ]
            ),
            (1, vec![b"read error: Input/output error\n".to_vec()])
        );
        assert!(output.bytes.is_empty());
        assert!(output.closed);
    }
}

#[test]
fn declared_health_allocations_are_checked_through_completion() {
    for limit in ["0", "2"] {
        let mut refusals = 0;
        let mut completed = false;
        for at in 0..500 {
            let mut input = Input {
                bytes: b"# skipped\nleft\tright\n1\t1\n2\t2\n3\t3\n4\t1.5\n5\tNA\n6\n",
                segment: 1,
                error: None,
            };
            let mut output = Output::default();
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = command(
                &mut input,
                &mut output,
                &[
                    "-C",
                    "-H",
                    "health",
                    "type",
                    "right",
                    "integer",
                    "nonmissing",
                    "2",
                    "required",
                    "5",
                    "width",
                    "3",
                    "examples",
                    limit,
                    "validate",
                ],
            );
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            if result == (1, Vec::new()) {
                let report = String::from_utf8(output.bytes).unwrap();
                assert!(report.contains("header_width_mismatch [declared, violation]"));
                assert!(report.contains("type_mismatch [declared, violation]: 1 cells"));
                assert!(report.contains(
                    "presence_mismatch [declared, violation]: 6 cells; expected required; field 5"
                ));
                assert!(!report.contains("field 3,"));
                assert!(!report.contains("field 4,"));
                assert!(output.closed);
                completed = true;
                break;
            }
            assert_eq!(
                result,
                (77, vec![b"command memory allocation failed\n".to_vec()]),
                "allocation {at}"
            );
            assert!(output.bytes.is_empty());
            refusals += 1;
        }
        assert!(completed && refusals > 0);
    }
}

#[test]
fn validation_status_preserves_write_and_close_failure_precedence() {
    for (code, expected) in [(28, 1), (5, 77)] {
        for close in [false, true] {
            let mut input = Input {
                bytes: b"1.5\nNA\n",
                segment: 1,
                error: None,
            };
            let mut output = if close {
                Output {
                    close_error: Some(code),
                    ..Default::default()
                }
            } else {
                Output {
                    write_error: Some(code),
                    ..Default::default()
                }
            };
            let (status, diagnostics) = command(
                &mut input,
                &mut output,
                &[
                    "health",
                    "type",
                    "1",
                    "integer",
                    "nonmissing",
                    "1",
                    "validate",
                ],
            );
            assert_eq!(status, expected);
            assert_eq!(diagnostics.len(), 1);
            assert!(output.closed);
            assert!(input.bytes.is_empty());
        }
    }
    let mut input = Input {
        bytes: b"1.5\n",
        segment: 1,
        error: None,
    };
    let mut output = Output {
        write_error: Some(5),
        ..Default::default()
    };
    let args = ["health", "type", "1", "integer", "validate"].map(std::ffi::OsString::from);
    let environment = Environment {
        locale: Default::default(),
        posixly_correct: false,
        grouping: None,
        sort_memory: None,
        pipe_grouping: None,
        terminal: Default::default(),
    };
    assert_eq!(
        run_in(
            &mut input,
            &mut output,
            &args,
            b"fastmash",
            &mut |_| false,
            environment
        )
        .ok()
        .unwrap(),
        1
    );
    assert!(output.closed);
}
