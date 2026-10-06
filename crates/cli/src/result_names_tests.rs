//! Command-boundary checks with controlled input and output failures.
use super::command_test_support::{Input, Output, command};
use super::*;

#[test]
fn result_names_survive_input_segmentation_and_short_writes() {
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes: b"value\n2\n4\n",
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
                &["-H", "--result-name=1:total:daily", "sum", "value"]
            ),
            (0, Vec::new())
        );
        assert_eq!(output.bytes, b"total:daily\n6\n");
        assert!(output.closed);
    }
}

#[test]
fn aliased_headers_still_validate_later_fields_after_a_write_failure() {
    for (code, status, secondary) in [
        (28, 1, b"write error\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        for csv in [false, true] {
            for (operation, selector) in [("sum", "2"), ("dotprod", "1:2"), ("dotprod", "2:1")] {
                let mut input = Input {
                    bytes: b"1\n",
                    segment: 1,
                    error: None,
                };
                let mut output = Output {
                    bytes: Vec::new(),
                    error: Some(code),
                    closed: false,
                };
                let mut args = vec![
                    "--header-out",
                    "--result-name=1:x",
                    "--result-name=2:y",
                    "sum",
                    "1",
                    operation,
                    selector,
                ];
                if csv {
                    args.insert(0, "--csv-out");
                }
                assert_eq!(
                    command(&mut input, &mut output, &args),
                    (
                        status,
                        vec![
                            b"invalid input: field 2 requested, line 1 has only 1 fields\n"
                                .to_vec(),
                            secondary.to_vec()
                        ]
                    ),
                    "{args:?}"
                );
                assert!(output.bytes.is_empty());
                assert!(output.closed);
            }
        }
    }
}

#[test]
fn result_names_retain_input_and_output_failure_precedence() {
    for (code, status, secondary) in [
        (28, 1, b"write error\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        let mut input = Input {
            bytes: b"2\n",
            segment: 1,
            error: Some(5),
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: Some(code),
            closed: false,
        };
        assert_eq!(
            command(
                &mut input,
                &mut output,
                &["--header-out", "--result-name=1:x", "sum", "1"]
            ),
            (
                status,
                vec![
                    b"read error: Input/output error\n".to_vec(),
                    secondary.to_vec()
                ]
            ),
        );
        assert!(output.closed);
    }
}

#[test]
fn naming_allocations_refuse_cleanly_at_the_command_boundary() {
    let mut completed = false;
    let mut refusals = 0;
    for at in 0..150 {
        let mut input = Input {
            bytes: b"1\n2\n",
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
        let (status, diagnostics) = command(
            &mut input,
            &mut output,
            &[
                "--header-out",
                "--result-name=2:last",
                "--result-name=1:first",
                "sum",
                "1",
                "count",
                "1",
            ],
        );
        command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        if status == 0 {
            assert!(diagnostics.is_empty());
            assert_eq!(output.bytes, b"first\tlast\n3\t2\n");
            completed = true;
            break;
        }
        assert_eq!(status, 77);
        assert_eq!(
            diagnostics,
            vec![b"command memory allocation failed\n".to_vec()]
        );
        refusals += 1;
    }
    assert!(completed && refusals > 0);
}

#[test]
fn csv_output_survives_segmentation_short_writes_and_transport_failures() {
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes: b"value\n2\n4\n",
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
                    "--csv-out",
                    "-H",
                    "--result-name=1:total,\"daily\"\r\n",
                    "sum",
                    "value"
                ]
            ),
            (0, Vec::new())
        );
        assert_eq!(output.bytes, b"\"total,\"\"daily\"\"\r\n\"\n6\n");
        assert!(output.closed);
    }
    for (code, status, secondary) in [
        (28, 1, b"write error\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n".as_slice()),
    ] {
        for (bytes, input_error, args, primary) in [
            (
                b"1\n".as_slice(),
                None,
                &[
                    "--csv-out",
                    "--header-out",
                    "--result-name=1:x,y",
                    "sum",
                    "1",
                    "sum",
                    "2",
                ][..],
                b"invalid input: field 2 requested, line 1 has only 1 fields\n".as_slice(),
            ),
            (
                b"2\n".as_slice(),
                Some(5),
                &["--csv-out", "--header-out", "sum", "1"][..],
                b"read error: Input/output error\n".as_slice(),
            ),
        ] {
            let mut input = Input {
                bytes,
                segment: 1,
                error: input_error,
            };
            let mut output = Output {
                bytes: Vec::new(),
                error: Some(code),
                closed: false,
            };
            assert_eq!(
                command(&mut input, &mut output, args),
                (status, vec![primary.to_vec(), secondary.to_vec()])
            );
            assert!(output.bytes.is_empty());
            assert!(output.closed);
        }
    }
}

#[test]
fn csv_finalization_reports_close_failures_and_keeps_status_precedence() {
    struct CloseFailure {
        output: Output,
        code: i32,
    }
    impl Write for CloseFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.output.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.output.flush()
        }
    }
    impl command_output::Transport for CloseFailure {
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
        for input_error in [None, Some(5)] {
            let mut input = Input {
                bytes: b"value\n2\n",
                segment: 1,
                error: input_error,
            };
            let mut output = CloseFailure {
                output: Output {
                    bytes: Vec::new(),
                    error: None,
                    closed: false,
                },
                code,
            };
            let mut expected = Vec::new();
            if input_error.is_some() {
                expected.push(b"read error: Input/output error\n".to_vec());
            }
            expected.push(diagnostic.to_vec());
            assert_eq!(
                command(
                    &mut input,
                    &mut output,
                    &["--csv-out", "-H", "sum", "value"]
                ),
                (status, expected)
            );
            assert_eq!(output.output.bytes, b"sum(value)\n2\n");
            assert!(output.output.closed);
        }
    }
}
