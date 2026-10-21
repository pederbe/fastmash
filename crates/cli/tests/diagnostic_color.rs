#![forbid(unsafe_code)]
//! Failure presentation preserves diagnostic bodies and transport behavior.
#[path = "support/descriptor_process.rs"]
mod descriptor_process;
#[path = "support/executable.rs"]
// The terminal helper selects the same executable through its private module.
#[allow(clippy::duplicate_mod)]
mod executable;
#[path = "support/full_output.rs"]
mod full_output;
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;
#[cfg(target_os = "macos")]
#[path = "support/test_process.rs"]
mod test_process;
use std::{
    fs::{self, File},
    os::{
        fd::OwnedFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    path::Path,
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
        let fixture = temp_dir::TempDir::new("diagnostic-help-full");
        let binary = executable::fastmash();
        #[cfg(target_os = "linux")]
        let mut command = std::process::Command::new(&binary);
        #[cfg(target_os = "macos")]
        let mut command = test_process::bounded(&binary, 30);
        #[cfg(target_os = "macos")]
        command.process_group(0);
        command
            .arg0("fastmash")
            .args([mode, "--help"])
            .env_clear()
            .env("LC_ALL", "C")
            .env("TERM", "xterm")
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null());
        let output = full_output::output(
            command,
            &binary,
            true,
            &fixture.0.with_extension("full-output"),
        );
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stderr.starts_with(PREFIX), mode == "--color=always");
        assert_eq!(
            terminal::strip_styles(&output.stderr),
            b"fastmash: write error: No space left on device\n"
        );
    }
}

#[test]
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
    let fixture = temp_dir::TempDir::new("diagnostic-failed-writes");
    let binary = executable::fastmash();
    // This sibling is outside ordinary fixture cleanup if native detach fails.
    let root = fixture.0.with_extension("full-output");
    let mut full = None;
    let mut next = 0;
    for mode in ["--color=always", "--color=never"] {
        for fault in ["closed", "read-only", "full", "broken"] {
            for (action, expected_signal) in [("--bad", None), ("sum", Some(13))] {
                let args = if action == "sum" {
                    vec![mode, action, "1"]
                } else {
                    vec![mode, action]
                };
                let directory = fixture.0.join(next.to_string());
                next += 1;
                fs::create_dir(&directory).unwrap();
                let mut files: [OwnedFd; 3] = [
                    File::open("/tmp").unwrap().into(),
                    File::create_new(directory.join("stdout")).unwrap().into(),
                    File::create_new(directory.join("stderr")).unwrap().into(),
                ];
                match fault {
                    "closed" => {}
                    "read-only" => files[2] = File::open("/dev/null").unwrap().into(),
                    "full" => {
                        let owner = full.get_or_insert_with(|| {
                            fastmash_corpus_driver::full_output::FullOutput::new(
                                Path::new(&binary),
                                &root,
                            )
                            .unwrap()
                        });
                        files[2] = owner.destination().unwrap().into();
                    }
                    "broken" => {
                        let (reader, writer) = std::io::pipe().unwrap();
                        drop(reader);
                        files[2] = writer.into();
                    }
                    _ => unreachable!(),
                }
                let output = descriptor_process::invoke_bound(
                    descriptor_process::BoundCommand {
                        binary: &binary,
                        arguments: &args,
                        files,
                        closed: if fault == "closed" { 4 } else { 0 },
                        environment: &[
                            ("LC_ALL", "C"),
                            ("TERM", "xterm"),
                            ("PATH", "/usr/bin:/bin"),
                        ],
                    },
                    &directory,
                    1 << 20,
                    1 << 20,
                )
                .unwrap()
                .into_output();
                if fault == "full" {
                    full.as_mut().unwrap().verify().unwrap();
                }
                fs::remove_dir_all(&directory).unwrap();
                assert!(!directory.try_exists().unwrap());
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
    if let Some(full) = full {
        full.finish().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    let path = fixture.0.clone();
    drop(fixture);
    assert!(!path.try_exists().unwrap());
}
