//! Strict CSV checks at the Command boundary with controlled transports.
use super::command_test_support::{Input, Output, command};
use super::*;

#[test]
fn csv_decoded_long_fields_preserve_bytes_empty_fields_and_original_locations() {
    let span = vec![b'x'; 257];
    let unquoted = [b"\xef\xbb\xbf\xff\0".as_slice(), &span].concat();
    let quoted = [&span[..], b",a\"b\r\n\0\xff"].concat();
    let bytes = [
        &unquoted[..],
        b",\"",
        &span,
        b",a\"\"b\r\n\0\xff\",,\r\n\n\"\"\nlast,",
    ]
    .concat();
    let expected: &[(u64, u64, &[&[u8]])] = &[
        (1, 1, &[&unquoted, &quoted, b"", b""]),
        (2, 3, &[b""]),
        (3, 4, &[b""]),
        (4, 5, &[b"last", b""]),
    ];
    for segment in [1, 2, 7, 127, 256, 257, 258, 259, 260, 511, 4096] {
        let mut input = Input {
            bytes: &bytes,
            segment,
            error: None,
        };
        let mut intake = csv_input::Intake::new();
        let mut record = csv_input::Record::default();
        for &(number, line, fields) in expected {
            assert!(matches!(intake.next(&mut input, &mut record), Ok(true)));
            assert_eq!(record.location.record, number);
            assert_eq!(record.location.line, line);
            assert_eq!(record.fields().collect::<Vec<_>>(), fields);
            assert!(record.field(fields.len() as u64 + 1).is_err());
        }
        assert!(matches!(intake.next(&mut input, &mut record), Ok(false)));
        assert!(intake.finish().is_ok());
    }
}

#[test]
fn csv_long_fields_keep_exact_syntax_reasons_and_detected_locations() {
    let span = vec![b'x'; 257];
    for (before, after, reason) in [
        (
            b"7,".as_slice(),
            b"\"x\n".as_slice(),
            "quote in unquoted field at physical line 3 byte 260",
        ),
        (
            b"7,\"",
            b"\" \n",
            "byte after closing quote at physical line 3 byte 262",
        ),
        (
            b"7,",
            b"\rx\n",
            "bare CR outside quoted field at physical line 3 byte 260",
        ),
        (
            b"7,",
            b"\r",
            "bare CR outside quoted field at physical line 3 byte 260",
        ),
        (b"7,\"", b"", "unexpected end of input in quoted field"),
    ] {
        let bytes = [b"2,\"one\r\ntwo\"\r\n".as_slice(), before, &span, after].concat();
        for segment in [1, 2, 7, 127, 256, 257, 258, 259, 260, 511, 4096] {
            let mut input = Input {
                bytes: &bytes,
                segment,
                error: None,
            };
            let mut output = Output {
                bytes: Vec::new(),
                error: None,
                closed: false,
            };
            assert_eq!(
                command(&mut input, &mut output, &["--csv", "sum", "1"]),
                (
                    1,
                    vec![
                        format!("invalid CSV: {reason}; record 2 starts at physical line 3\n")
                            .into_bytes()
                    ]
                ),
                "segment {segment}"
            );
            assert!(output.bytes.is_empty());
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_later_long_field_syntax_preserves_streaming_and_sorted_error_order() {
    let span = vec![b'x'; 257];
    let bytes = [
        b"key,value,note\nz,bad,valid\na,2,\"".as_slice(),
        &span,
        b"\"!\n",
    ]
    .concat();
    for sorted in [false, true] {
        for segment in [1, 2, 7, 257, 258, 4096] {
            let mut input = Input {
                bytes: &bytes,
                segment,
                error: None,
            };
            let mut output = Output {
                bytes: Vec::new(),
                error: None,
                closed: false,
            };
            let args = if sorted {
                &["--csv", "-H", "-s", "-g", "key", "sum", "value"][..]
            } else {
                &["--csv", "-H", "sum", "value"]
            };
            let diagnostic = if sorted {
                b"invalid CSV: byte after closing quote at physical line 3 byte 264; record 3 starts at physical line 3\n".as_slice()
            } else {
                b"invalid numeric value in line 2 field 2: 'bad'\nCSV record 2 starts at physical line 2\n"
            };
            assert_eq!(
                command(&mut input, &mut output, args),
                (1, vec![diagnostic.to_vec()])
            );
            assert_eq!(
                output.bytes,
                if sorted {
                    b"GroupBy(key),sum(value)\n".as_slice()
                } else {
                    b"sum(value)\n"
                }
            );
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_multiple_numeric_errors_follow_sorted_keys_with_original_locations() {
    let span = vec![b'x'; 257];
    let bytes = [
        b"key,value,note\nz,first,".as_slice(),
        &span,
        b"\na,second,\"two\nlines\"\n",
    ]
    .concat();
    for locale in ["C", "en_US.UTF-8"] {
        for target in ["67108864", "1"] {
            for segment in [1, 2, 7, 257, 258, 4096] {
                let mut input = Input {
                    bytes: &bytes,
                    segment,
                    error: None,
                };
                let mut output = Output {
                    bytes: Vec::new(),
                    error: None,
                    closed: false,
                };
                assert_eq!(
            command_test_support::command_in(&mut input, &mut output, &["--csv", "-H", "-s", "-g", "key", "sum", "value"], sort_environment(locale, target)),
            (1, vec![b"invalid numeric value in line 3 field 2: 'second'\nCSV record 3 starts at physical line 3\n".to_vec()])
        );
                assert_eq!(output.bytes, b"GroupBy(key),sum(value)\n");
                assert!(output.closed);
            }
        }
    }
}

#[test]
fn csv_sort_retains_complete_records_across_segmented_intake_and_short_writes() {
    let bytes = b"key,value,note\r\nb,1,\"b\r\nrecord\"\na,2,first\n\"a\",3,\"last\"\"note\"\n";
    for segment in 1..=bytes.len() {
        let mut input = Input {
            bytes,
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(
            command(
                &mut input,
                &mut output,
                &[
                    "--csv", "-H", "-s", "-g", "1", "sum", "2", "first", "3", "last", "3"
                ]
            ),
            (0, vec![])
        );
        assert_eq!(output.bytes, b"GroupBy(key),sum(value),first(note),last(note)\na,5,first,\"last\"\"note\"\nb,1,\"b\r\nrecord\",\"b\r\nrecord\"\n");
        assert!(output.closed);
    }
}

#[test]
fn csv_sort_read_failure_keeps_io_context_and_finalization_precedence() {
    for locale in ["C", "en_US.UTF-8"] {
        for target in ["67108864", "1"] {
            for header in [false, true] {
                for (error, status, secondary) in [
                    (None, 1, None),
                    (Some(28), 1, Some(b"write error\n".as_slice())),
                    (
                        Some(5),
                        77,
                        Some(b"unsupported output I/O error\n".as_slice()),
                    ),
                ] {
                    let bytes = if header {
                        b"key,value\na,2\n\"unfinished".as_slice()
                    } else {
                        b"a,2\n\"unfinished".as_slice()
                    };
                    let mut input = Input {
                        bytes,
                        segment: 1,
                        error: Some(5),
                    };
                    let mut output = Output {
                        bytes: Vec::new(),
                        error,
                        closed: false,
                    };
                    let mut args = vec!["--csv", "-s", "-g", "1", "sum", "2"];
                    if header {
                        args.push("-H");
                    }
                    let mut diagnostics = vec![b"read error: Input/output error\n".to_vec()];
                    if let Some(message) = secondary.filter(|_| header) {
                        diagnostics.push(message.to_vec());
                    }
                    assert_eq!(
                        command_test_support::command_in(
                            &mut input,
                            &mut output,
                            &args,
                            sort_environment(locale, target)
                        ),
                        (if header { status } else { 1 }, diagnostics)
                    );
                    assert_eq!(
                        output.bytes,
                        if header && error.is_none() {
                            b"GroupBy(key),sum(value)\n".as_slice()
                        } else {
                            b""
                        }
                    );
                    assert!(output.closed);
                }
            }
        }
    }
}

#[test]
fn csv_language_large_key_scratch_keeps_small_groups_and_original_locations() {
    let key = vec![b'z'; 65537];
    let bytes = [key.as_slice(), b",1\n", &b"a,2\n".repeat(80)].concat();
    let expected = [b"a,160\n".as_slice(), &key, b",1\n"].concat();
    for target in ["67108864", "32768", "4096", "1"] {
        let mut input = Input {
            bytes: &bytes,
            segment: 257,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(
            command_test_support::command_in(
                &mut input,
                &mut output,
                &["--csv", "-i", "-s", "-g", "1", "sum", "2"],
                sort_environment("en_US.UTF-8", target)
            ),
            (0, vec![])
        );
        assert_eq!(output.bytes, expected);
        assert!(output.closed);
    }
    for target in ["67108864", "1"] {
        let mut input = Input {
            bytes: b"value,key\n1,z\n2\n",
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(command_test_support::command_in(&mut input, &mut output,
            &["--csv", "-H", "-s", "-g", "key", "count", "value"],
            sort_environment("en_US.UTF-8", target)),
            (1, vec![b"invalid input: field 2 requested, line 3 has only 1 fields\nCSV record 3 starts at physical line 3\n".to_vec()]));
        assert_eq!(output.bytes, b"GroupBy(key),count(value)\n");
        assert!(output.closed);
    }
}

#[test]
fn csv_sort_retention_reservations_fail_cleanly_through_the_command() {
    let bytes = b"key,value\nb,1\na,2\na,3\nb,4\n";
    for locale in ["C", "en_US.UTF-8"] {
        for target in ["67108864", "1"] {
            let mut completed = false;
            for at in 0..200 {
                let mut input = Input {
                    bytes,
                    segment: 2,
                    error: None,
                };
                let mut output = Output {
                    bytes: Vec::new(),
                    error: None,
                    closed: false,
                };
                command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
                let result = command_test_support::command_in(
                    &mut input,
                    &mut output,
                    &["--csv", "-H", "-s", "-g", "1", "sum", "2"],
                    sort_environment(locale, target),
                );
                command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
                if result.0 == 0 {
                    assert_eq!(result.1, vec![] as Vec<Vec<u8>>);
                    assert_eq!(output.bytes, b"GroupBy(key),sum(value)\na,5\nb,5\n");
                    assert!(output.closed);
                    completed = true;
                    break;
                }
                assert_eq!(
                    result,
                    (77, vec![b"command memory allocation failed\n".to_vec()])
                );
                assert!(b"GroupBy(key),sum(value)\n".starts_with(&output.bytes));
            }
            assert!(completed);
        }
    }
}

#[test]
fn csv_sort_small_targets_spill_records_and_language_keys() {
    for (locale, target, bytes, expected) in [
        ("C", "4096", b"\n".as_slice(), Some(b"\"\",1\n".as_slice())),
        ("C", "4096", &[b'\n'; 100], Some(b"\"\",100\n")),
        ("en_US.UTF-8", "4096", b"a\n", Some(b"a,1\n")),
        ("en_US.UTF-8", "65536", b"a\n", Some(b"a,1\n")),
    ] {
        let environment = Environment {
            locale: locale::Policy::resolve(|key| (key == "LC_ALL").then(|| locale.into())),
            posixly_correct: false,
            grouping: None,
            sort_memory: Some(target.into()),
            pipe_grouping: None,
            terminal: Default::default(),
        };
        let mut input = Input {
            bytes,
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        let result = command_test_support::command_in(
            &mut input,
            &mut output,
            &["--csv", "-s", "-g", "1", "count", "1"],
            environment,
        );
        assert_eq!(result, (0, vec![]));
        assert_eq!(output.bytes, expected.unwrap());
        assert!(output.closed);
    }
}

#[test]
fn csv_sort_oversized_record_then_small_records_preserves_complete_final_groups() {
    let long = vec![b'x'; 65537];
    let bytes = [
        b"z,1,".as_slice(),
        &long,
        b"\n",
        &b"a,1,small\n".repeat(200),
    ]
    .concat();
    let expected = [
        b"a,200,small,small\nz,1,".as_slice(),
        &long,
        b",",
        &long,
        b"\n",
    ]
    .concat();
    for locale in ["C", "en_US.UTF-8"] {
        for target in ["67108864", "32768", "4096", "1"] {
            let environment = sort_environment(locale, target);
            let mut input = Input {
                bytes: &bytes,
                segment: 257,
                error: None,
            };
            let mut output = Output {
                bytes: Vec::new(),
                error: None,
                closed: false,
            };
            assert_eq!(
                command_test_support::command_in(
                    &mut input,
                    &mut output,
                    &[
                        "--csv", "-s", "-g", "1", "sum", "2", "first", "3", "last", "3"
                    ],
                    environment,
                ),
                (0, vec![]),
            );
            assert_eq!(output.bytes, expected, "target {target}");
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_sorted_field_failure_precedes_simultaneous_output_failure() {
    for (code, status, message) in [
        (28, 1, b"write error\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        let mut input = Input {
            bytes: b"key,value\nz,2\na,bad\n",
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: Some(code),
            closed: false,
        };
        assert_eq!(command(&mut input, &mut output, &["--csv", "-H", "-s", "-g", "1", "sum", "2"]),
            (status, vec![b"invalid numeric value in line 3 field 2: 'bad'\nCSV record 3 starts at physical line 3\n".to_vec(), message.to_vec()]));
        assert!(output.closed);
    }
}

#[test]
fn csv_records_survive_every_quote_crlf_boundary_and_short_writes() {
    let bytes = b"\"a,b\",unused\r\n\"x\"\"y\n\0z\",tail\n\"\",last";
    for segment in 1..=bytes.len() {
        let mut input = Input {
            bytes,
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(
            command(
                &mut input,
                &mut output,
                &["--csv", "cut", "1", "base64", "1"]
            ),
            (0, vec![])
        );
        assert_eq!(
            output.bytes,
            b"\"a,b\",YSxi\n\"x\"\"y\n\",eCJ5CgB6\n\"\",\"\"\n"
        );
        assert!(output.closed);
    }
}

#[test]
fn csv_malformed_unused_fields_fail_before_that_records_results() {
    for segment in [1, 2, 3, 7, 128] {
        let mut input = Input {
            bytes: b"1,\"x\ny\"!\n",
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(command(&mut input, &mut output, &["--csv", "--header-out", "cut", "1"]),
            (1, vec![b"invalid CSV: byte after closing quote at physical line 2 byte 3; record 1 starts at physical line 1\n".to_vec()]));
        assert!(output.bytes.is_empty());
        assert!(output.closed);
    }
}

#[test]
fn csv_io_failure_is_not_synthetic_eof_and_transport_precedence_is_preserved() {
    for bytes in [b"\"unfinished".as_slice(), b"2,\"valid\"\n\"unfinished"] {
        for (error, status, secondary) in [
            (None, 1, None),
            (Some(28), 1, Some(b"write error\n".as_slice())),
            (
                Some(5),
                77,
                Some(b"unsupported output I/O error\n".as_slice()),
            ),
        ] {
            let mut input = Input {
                bytes,
                segment: 1,
                error: Some(5),
            };
            let mut output = Output {
                bytes: Vec::new(),
                error,
                closed: false,
            };
            let mut expected = vec![b"read error: Input/output error\n".to_vec()];
            if let Some(secondary) = secondary.filter(|_| bytes.starts_with(b"2")) {
                expected.push(secondary.to_vec());
            }
            let status = if bytes.starts_with(b"2") { status } else { 1 };
            assert_eq!(
                command(&mut input, &mut output, &["--csv", "sum", "1"]),
                (status, expected)
            );
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_field_validation_precedes_simultaneous_output_failure() {
    for (code, status, secondary) in [
        (28, 1, b"write error\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        let mut input = Input {
            bytes: b"header\n1\n",
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: Some(code),
            closed: false,
        };
        assert_eq!(command(&mut input, &mut output, &["--csv", "-H", "--result-name=1:x", "sum", "1", "sum", "2"]),
            (status, vec![b"invalid input: field 2 requested, line 1 has only 1 fields\nCSV record 1 starts at physical line 1\n".to_vec(), secondary.to_vec()]));
        assert!(output.closed);
    }
}

#[test]
fn csv_record_and_field_growth_refuse_cleanly_without_partial_record_emission() {
    let mut completed = false;
    let bytes = [b"\"".as_slice(), &vec![b'x'; 2000], b"\",,,\"\"\n"].concat();
    for at in 0..200 {
        let mut input = Input {
            bytes: &bytes,
            segment: 3,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
        let result = command(&mut input, &mut output, &["--csv", "cut", "1-4"]);
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        if result.0 == 0 {
            assert!(output.closed);
            assert!(result.1.is_empty());
            assert_eq!(
                output.bytes,
                [&vec![b'x'; 2000][..], b",\"\",\"\",\"\"\n"].concat()
            );
            completed = true;
            break;
        }
        assert_eq!(
            result,
            (77, vec![b"command memory allocation failed\n".to_vec()])
        );
        assert!(output.bytes.is_empty());
    }
    assert!(completed);
}

#[test]
fn csv_input_finalization_preserves_close_failure_status_and_io_context() {
    struct Closing {
        output: Output,
        code: i32,
    }
    impl Write for Closing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.output.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.output.flush()
        }
    }
    impl command_output::Transport for Closing {
        fn buffering(&self) -> (usize, bool) {
            self.output.buffering()
        }
        fn close(&mut self) -> io::Result<()> {
            self.output.closed = true;
            Err(io::Error::from_raw_os_error(self.code))
        }
    }
    for (code, status, diagnostic) in [
        (28, 1, b"write error: No space left on device\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        for error in [None, Some(5)] {
            for sorted in [false, true] {
                let mut input = Input {
                    bytes: b"\"amount,raw\"\r\n2\n",
                    segment: 1,
                    error,
                };
                let mut output = Closing {
                    output: Output {
                        bytes: Vec::new(),
                        error: None,
                        closed: false,
                    },
                    code,
                };
                let mut expected = Vec::new();
                if error.is_some() {
                    expected.push(b"read error: Input/output error\n".to_vec());
                }
                expected.push(diagnostic.to_vec());
                let args = if sorted {
                    &["--csv", "-H", "-s", "-g", "1", "sum", "1"][..]
                } else {
                    &["--csv", "-H", "sum", "1"][..]
                };
                assert_eq!(command(&mut input, &mut output, args), (status, expected));
                let expected: &[u8] = if !sorted {
                    b"\"sum(amount,raw)\"\n2\n".as_slice()
                } else if error.is_some() {
                    b"\"GroupBy(amount,raw)\",\"sum(amount,raw)\"\n"
                } else {
                    b"\"GroupBy(amount,raw)\",\"sum(amount,raw)\"\n2,2\n"
                };
                assert_eq!(output.output.bytes, expected);
                assert!(output.output.closed);
            }
        }
    }
}

#[test]
fn csv_groups_and_full_records_survive_segmented_swaps_and_short_writes() {
    let bytes =
        b"key,value,note\r\n\"a,b\",1,\"first\nrecord\"\n\"a,b\",2,\"x\"\"y\r\n\0z\"\r\nc,3,tail";
    for segment in 1..=bytes.len() {
        let mut input = Input {
            bytes,
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(
            command(
                &mut input,
                &mut output,
                &[
                    "--csv",
                    "-H",
                    "--full",
                    "-g",
                    "key",
                    "--result-name=1:total",
                    "sum",
                    "value",
                    "last",
                    "note"
                ]
            ),
            (0, vec![])
        );
        assert_eq!(output.bytes, b"key,value,note,total,last(note)\n\"a,b\",2,\"x\"\"y\r\n\0z\",3,\"x\"\"y\r\n\"\nc,3,tail,3,tail\n");
        assert!(output.closed);
    }
}

#[test]
fn csv_group_late_read_and_output_failures_keep_the_finished_representative() {
    for full in [false, true] {
        for (error, status, secondary) in [
            (None, 1, None),
            (Some(28), 1, Some(b"write error\n".as_slice())),
            (
                Some(5),
                77,
                Some(b"unsupported output I/O error\n".as_slice()),
            ),
        ] {
            let mut input = Input {
                bytes: b"\"a,b\",1,\"two\nlines\"\n\"unfinished",
                segment: 1,
                error: Some(5),
            };
            let mut output = Output {
                bytes: Vec::new(),
                error,
                closed: false,
            };
            let mut args = vec!["--csv", "-g", "1", "sum", "2"];
            if full {
                args.insert(0, "--full");
            }
            let mut expected = vec![b"read error: Input/output error\n".to_vec()];
            if let Some(secondary) = secondary {
                expected.push(secondary.to_vec());
            }
            assert_eq!(command(&mut input, &mut output, &args), (status, expected));
            if error.is_none() {
                assert_eq!(
                    output.bytes,
                    if full {
                        b"\"a,b\",1,\"two\nlines\",1\n".as_slice()
                    } else {
                        b"\"a,b\",1\n"
                    }
                );
            }
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_malformed_later_group_never_collects_or_emits_its_partial_record() {
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes: b"a,1,\"two\nlines\"\nb,2,x\nc,3,\"bad\"!\n",
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(command(&mut input, &mut output, &["--csv", "-g", "1", "sum", "2"]),
            (1, vec![b"invalid CSV: byte after closing quote at physical line 4 byte 10; record 3 starts at physical line 4\n".to_vec()]));
        assert_eq!(output.bytes, b"a,1\n");
        assert!(output.closed);
    }
}

#[test]
fn csv_owned_group_and_full_growth_refuse_without_corrupting_completed_groups() {
    let bytes = [
        b"a,1,\"".as_slice(),
        &vec![b'x'; 2000],
        b"\"\n\"a\",2,short,\n\"b\",3,last\n",
    ]
    .concat();
    let first = b"a,2,short,\"\",2,short\n";
    let expected = [first.as_slice(), b"b,3,last,1,last\n"].concat();
    let mut completed = false;
    for at in 0..200 {
        let mut input = Input {
            bytes: &bytes,
            segment: 3,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
        let result = command(
            &mut input,
            &mut output,
            &["--csv", "--full", "-g", "1", "count", "2", "last", "3"],
        );
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        if result.0 == 0 {
            assert!(output.closed);
            assert!(result.1.is_empty());
            assert_eq!(output.bytes, expected);
            completed = true;
            break;
        }
        assert_eq!(result.0, 77, "{result:?}");
        assert!(
            result
                .1
                .iter()
                .any(|message| message.ends_with(b"memory allocation failed\n")),
            "{result:?}"
        );
        assert!(expected.starts_with(&output.bytes), "{:?}", output.bytes);
    }
    assert!(completed);
}

fn sort_environment(locale: &str, target: &str) -> Environment {
    Environment {
        locale: locale::Policy::resolve(|key| (key == "LC_ALL").then(|| locale.into())),
        posixly_correct: false,
        grouping: None,
        sort_memory: Some(target.into()),
        pipe_grouping: None,
        terminal: Default::default(),
    }
}

#[test]
fn csv_spill_command_handles_temporary_write_read_and_merge_failures() {
    use projected_sort::{RunFault, fail_run};
    let bytes = [
        b"key,value\n".as_slice(),
        b"b,1\na,2\n".repeat(12).as_slice(),
    ]
    .concat();
    for locale in ["C", "en_US.UTF-8"] {
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
                let mut input = Input {
                    bytes: &bytes,
                    segment: 2,
                    error: None,
                };
                let mut output = Output {
                    bytes: Vec::new(),
                    error: output_error,
                    closed: false,
                };
                fail_run(Some((fault, at, 5)));
                let result = command_test_support::command_in(
                    &mut input,
                    &mut output,
                    &["--csv", "-H", "-s", "-g", "1", "sum", "2"],
                    sort_environment(locale, "1"),
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
                assert!(b"GroupBy(key),sum(value)\na,24\nb,12\n".starts_with(&output.bytes));
                assert!(output.closed);
            }
        }
    }
}

#[test]
fn csv_spill_checked_allocations_and_final_output_keep_failure_precedence() {
    let bytes = [
        b"key,value\n".as_slice(),
        b"b,1\na,2\n".repeat(12).as_slice(),
    ]
    .concat();
    let expected = b"GroupBy(key),sum(value)\na,24\nb,12\n";
    let mut completed = false;
    for at in 0..2000 {
        let mut input = Input {
            bytes: &bytes,
            segment: 2,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
        let result = command_test_support::command_in(
            &mut input,
            &mut output,
            &["--csv", "-H", "-s", "-g", "1", "sum", "2"],
            sort_environment("C", "1"),
        );
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        assert!(expected.starts_with(&output.bytes));
        if result.0 == 0 {
            assert!(output.closed);
            assert!(result.1.is_empty());
            assert_eq!(output.bytes, expected);
            completed = true;
            break;
        }
        assert_eq!(
            result,
            (77, vec![b"command memory allocation failed\n".to_vec()])
        );
    }
    assert!(completed);
    for (error, status, message) in [
        (28, 1, b"write error: No space left on device\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n"),
    ] {
        let mut input = Input {
            bytes: &bytes,
            segment: 2,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: Some(error),
            closed: false,
        };
        assert_eq!(
            command_test_support::command_in(
                &mut input,
                &mut output,
                &["--csv", "-H", "-s", "-g", "1", "sum", "2"],
                sort_environment("C", "1")
            ),
            (status, vec![message.to_vec()])
        );
        assert!(output.closed);
    }
}
