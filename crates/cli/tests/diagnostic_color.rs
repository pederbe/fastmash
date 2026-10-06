//! Failure presentation preserves diagnostic bodies and transport behavior.
#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;
#[path = "support/terminal.rs"]
mod terminal;
use std::{
    fs::File,
    os::unix::process::{CommandExt, ExitStatusExt},
    process::Stdio,
};

const PREFIX: &[u8] = b"\x1b[31mfastmash: \x1b[0m";

const PROGRAM: terminal::Program = terminal::Program;

#[test]
fn argument_input_and_refusal_diagnostics_style_only_the_existing_prefix() {
    let fixtures: &[(&[&str], &[u8], i32)] = &[
        (&["--not-an-option"], b"", 1),
        (&["not-an-operation", "1"], b"", 1),
        (&["sum", "1"], b"\xff\n", 1),
        (&["--csv", "sum", "1"], b"\"unterminated\n", 1),
        (&["--sort-cmd=/bin/sort", "sum", "1"], b"", 77),
        (&["wmean", "1:2"], b"1\t-1\n", 1),
    ];
    for &(args, input, status) in fixtures {
        let mut plain_args = vec!["--color=never"];
        plain_args.extend(args);
        let plain = PROGRAM.invoke(&plain_args, input, &[], false, false);
        let mut colored_args = vec!["--color=always"];
        colored_args.extend(args);
        let colored = PROGRAM.invoke(&colored_args, input, &[], false, false);
        assert_eq!(plain.status.code(), Some(status), "{args:?}: {plain:?}");
        assert_eq!(colored.status, plain.status, "{args:?}");
        assert_eq!(colored.stdout, plain.stdout, "{args:?}");
        let body = plain.stderr.strip_prefix(b"fastmash: ").unwrap();
        assert_eq!(colored.stderr, [PREFIX, body].concat(), "{args:?}");
        assert_eq!(terminal::strip_styles(&colored.stderr), plain.stderr);
    }
}

#[test]
fn table_check_excerpt_values_stay_plain_before_the_failure_prefix() {
    let args = ["check"];
    let input = b"a\t\xff\nb\n";
    let plain = PROGRAM.invoke(&args, input, &[], false, false);
    let colored = PROGRAM.invoke(&["--color=always", "check"], input, &[], false, true);
    assert_eq!(colored.status.code(), Some(1));
    assert_eq!(colored.status, plain.status);
    let marker = plain
        .stderr
        .windows(b"fastmash: ".len())
        .position(|bytes| bytes == b"fastmash: ")
        .unwrap();
    assert!(marker > 0);
    assert_eq!(colored.stderr[..marker], plain.stderr[..marker]);
    assert_eq!(
        &colored.stderr[marker..],
        [PREFIX, &plain.stderr[marker + b"fastmash: ".len()..]].concat()
    );
}

#[test]
fn automatic_diagnostics_use_stderr_eligibility_independently_of_stdout() {
    for stdout_terminal in [false, true] {
        for stderr_terminal in [false, true] {
            for (environment, eligible) in [
                (vec![], true),
                (vec![("NO_COLOR", "")], true),
                (vec![("NO_COLOR", "0")], false),
                (vec![("TERM", "dumb")], false),
            ] {
                let output = PROGRAM.invoke(
                    &["sum", "1"],
                    b"bad\n",
                    &environment,
                    stdout_terminal,
                    stderr_terminal,
                );
                assert_eq!(output.status.code(), Some(1));
                assert_eq!(
                    output.stderr.starts_with(PREFIX),
                    stderr_terminal && eligible,
                    "stdout={stdout_terminal}, stderr={stderr_terminal}, {environment:?}"
                );
                assert_eq!(
                    terminal::strip_styles(&output.stderr),
                    b"fastmash: invalid numeric value in line 1 field 1: 'bad'\n"
                );
                assert!(output.stdout.is_empty());
            }
        }
    }
    for (mode, styled) in [
        ("--color=always", true),
        ("--color=never", false),
        ("--no-color", false),
    ] {
        for stderr_terminal in [false, true] {
            let output = PROGRAM.invoke(
                &[mode, "sum", "1"],
                b"bad\n",
                &[("TERM", "dumb"), ("NO_COLOR", "1")],
                false,
                stderr_terminal,
            );
            assert_eq!(output.stderr.starts_with(PREFIX), styled);
        }
    }
}

#[test]
fn option_failures_keep_the_color_reached_by_the_original_scan() {
    for (args, styled) in [
        (vec!["--color=always", "--color=wrong"], true),
        (vec!["--color=never", "--color=wrong"], false),
        (vec!["--color=always", "--no-color", "--bad"], false),
        (vec!["--no-color", "--color=always", "--bad"], true),
        (
            vec!["--color=always", "--sort-cmd=/bin/sort", "--no-color"],
            true,
        ),
        (vec!["--bad", "--color=always"], false),
        (vec!["-t", "--color=always"], false),
        (vec!["--", "--color=always"], false),
    ] {
        let output = PROGRAM.invoke(&args, b"", &[], false, false);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stderr.starts_with(PREFIX), styled, "{args:?}");
    }
    let stopped = PROGRAM.invoke(
        &["sum", "1", "--color=always"],
        b"",
        &[("POSIXLY_CORRECT", "")],
        false,
        false,
    );
    assert!(!stopped.status.success());
    assert!(!stopped.stderr.contains(&0x1b));
}

#[test]
fn help_output_failures_retain_selected_diagnostic_color() {
    for mode in ["--color=always", "--color=never"] {
        let output = PROGRAM
            .command(&[mode, "--help"], &[])
            .stdin(Stdio::null())
            .full_stdout(true)
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stderr.starts_with(PREFIX), mode == "--color=always");
        assert_eq!(
            terminal::strip_styles(&output.stderr),
            b"fastmash: write error: No space left on device\n"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn child_sort_diagnostics_are_forwarded_without_generated_styling() {
    let args = ["-H", "-s", "-g", "key", "geomean", "value"];
    let plain = PROGRAM.invoke(&args, b"", &[("FASTMASH_GROUPING", "sort")], false, false);
    let mut colored_args = vec!["--color=always"];
    colored_args.extend(args);
    let colored = PROGRAM.invoke(
        &colored_args,
        b"",
        &[("FASTMASH_GROUPING", "sort")],
        false,
        true,
    );
    assert_eq!(colored.status.code(), Some(1), "{colored:?}");
    assert_eq!(colored.status, plain.status);
    assert_eq!(colored.stdout, plain.stdout);
    let marker = plain
        .stderr
        .windows(b"fastmash: ".len())
        .position(|bytes| bytes == b"fastmash: ")
        .unwrap();
    assert!(marker > 0, "expected the system sort's diagnostic first");
    assert_eq!(colored.stderr[..marker], plain.stderr[..marker]);
    assert_eq!(
        &colored.stderr[marker..],
        [PREFIX, &plain.stderr[marker + b"fastmash: ".len()..]].concat()
    );
}

#[test]
#[cfg(target_os = "macos")]
fn native_missing_header_diagnostic_receives_the_selected_style() {
    let plain = PROGRAM.invoke(
        &["-H", "-s", "-g", "key", "geomean", "value"],
        b"",
        &[],
        false,
        false,
    );
    let styled = PROGRAM.invoke(
        &[
            "--color=always",
            "-H",
            "-s",
            "-g",
            "key",
            "geomean",
            "value",
        ],
        b"",
        &[],
        false,
        true,
    );
    assert_eq!(plain.status.code(), Some(1));
    assert_eq!(
        plain.stderr,
        b"fastmash: missing input header for named grouping key\n"
    );
    assert_eq!(styled.status, plain.status);
    assert_eq!(styled.stdout, plain.stdout);
    assert_eq!(
        styled.stderr,
        [PREFIX, b"missing input header for named grouping key\n"].concat()
    );
}

#[test]
fn failed_diagnostic_writes_preserve_exit_status_and_sigpipe_behavior() {
    for mode in ["--color=always", "--color=never"] {
        for fault in ["closed", "read-only", "full", "broken"] {
            for (action, expected_signal) in [("--bad", None), ("sum", Some(13))] {
                let args = if action == "sum" {
                    vec![mode, action, "1"]
                } else {
                    vec![mode, action]
                };
                let mut command = PROGRAM.command(&args, &[]);
                command.stdin(File::open("/tmp").unwrap());
                match fault {
                    "closed" => {
                        // SAFETY: only async-signal-safe close runs in the child.
                        unsafe {
                            command.pre_exec(|| {
                                if libc::close(2) < 0 {
                                    return Err(std::io::Error::last_os_error());
                                }
                                Ok(())
                            });
                        }
                    }
                    "read-only" => {
                        command.stderr(File::open("/dev/null").unwrap());
                    }
                    "full" => {
                        command.full_stderr();
                    }
                    "broken" => {
                        let (reader, writer) = std::io::pipe().unwrap();
                        drop(reader);
                        command.stderr(writer);
                    }
                    _ => unreachable!(),
                }
                let output = command.output().unwrap();
                if fault == "broken" && expected_signal.is_some() {
                    assert_eq!(output.status.signal(), expected_signal, "{args:?}: {fault}");
                } else {
                    assert_eq!(output.status.code(), Some(1), "{args:?}: {fault}");
                }
                assert!(output.stdout.is_empty());
                assert!(output.stderr.is_empty());
            }
        }
    }
}
