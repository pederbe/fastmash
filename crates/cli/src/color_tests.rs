//! Help through the controlled Command transport, including failed completion.
use super::*;
use command_test_support::{Input, command, command_in};

struct Output {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    fail_flush: bool,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.fail_at.map_or(usize::MAX, |at| at - self.bytes.len());
        if remaining == 0 {
            return Err(io::Error::from_raw_os_error(28));
        }
        let count = bytes.len().min(2).min(remaining);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::from_raw_os_error(28))
        } else {
            Ok(())
        }
    }
}
impl command_output::Transport for Output {
    fn buffering(&self) -> (usize, bool) {
        (1, false)
    }
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn input() -> Input<'static> {
    Input {
        bytes: b"unread",
        segment: 1,
        error: Some(5),
    }
}

#[test]
fn help_keeps_input_unread_and_reports_short_write_and_flush_failures() {
    for mode in ["--color=never", "--color=always"] {
        let mut expected = Output {
            bytes: Vec::new(),
            fail_at: None,
            fail_flush: false,
        };
        let mut reader = input();
        let (status, diagnostics) = command(&mut reader, &mut expected, &[mode, "--help"]);
        assert_eq!(status, 0);
        assert!(diagnostics.is_empty());
        assert_eq!(reader.bytes, b"unread");
        for fail_at in [0, 2, 8, 100, expected.bytes.len() - 1] {
            let mut failed = Output {
                bytes: Vec::new(),
                fail_at: Some(fail_at),
                fail_flush: false,
            };
            let (status, diagnostics) = command(&mut input(), &mut failed, &[mode, "--help"]);
            assert_eq!(status, 1);
            assert_eq!(failed.bytes, expected.bytes[..fail_at]);
            assert_eq!(
                diagnostics,
                vec![b"write error: No space left on device\n".to_vec()]
            );
        }
        let mut failed = Output {
            bytes: Vec::new(),
            fail_at: None,
            fail_flush: true,
        };
        let (status, diagnostics) = command(&mut input(), &mut failed, &[mode, "--help"]);
        assert_eq!(status, 1);
        assert_eq!(failed.bytes, expected.bytes);
        assert_eq!(
            diagnostics,
            vec![b"write error: No space left on device\n".to_vec()]
        );
    }
}

#[test]
fn controlled_help_detection_uses_stdout_even_when_stderr_differs() {
    for (stdout_terminal, stderr_terminal) in [(true, false), (false, true)] {
        let environment = Environment {
            locale: Default::default(),
            posixly_correct: false,
            grouping: None,
            sort_memory: None,
            pipe_grouping: None,
            terminal: terminal_style::Detection {
                stdout_terminal,
                stderr_terminal,
                suppressed: false,
            },
        };
        let mut output = Output {
            bytes: Vec::new(),
            fail_at: None,
            fail_flush: false,
        };
        let (status, diagnostics) = command_in(&mut input(), &mut output, &["--help"], environment);
        assert_eq!(status, 0);
        assert!(diagnostics.is_empty());
        assert_eq!(output.bytes.contains(&0x1b), stdout_terminal);
    }
}
