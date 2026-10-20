#![forbid(unsafe_code)]
//! Executed-command checks for closed and incorrectly opened standard streams.
#[path = "support/descriptor_process.rs"]
mod descriptor_process;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/temp_dir.rs"]
mod temp_dir;
use descriptor_process::{Streams, invoke};
use nix::sys::wait::WaitStatus;
#[test]
fn ordinary_standard_streams_count_records() {
    check(Streams::Ordinary, 0, b"2\n", b"");
}

#[test]
fn closed_standard_stream_combinations_keep_their_original_failure() {
    for closed in 1u8..8 {
        let status = i32::from(closed & 3 != 0);
        let stdout: &[u8] = if closed & 3 == 0 { b"2\n" } else { b"" };
        let stderr: &[u8] = if closed & 4 != 0 {
            b""
        } else if closed & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        check(Streams::Closed(closed), status, stdout, stderr);
    }
}

#[test]
fn wrong_access_stream_combinations_keep_their_original_failure() {
    for wrong in 1u8..8 {
        let status = i32::from(wrong & 3 != 0);
        let stdout: &[u8] = if wrong & 3 == 0 { b"2\n" } else { b"" };
        let stderr: &[u8] = if wrong & 4 != 0 {
            b""
        } else if wrong & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        check(Streams::WrongAccess(wrong), status, stdout, stderr);
    }
}

fn check(streams: Streams, status: i32, stdout: &[u8], stderr: &[u8]) {
    let directory = temp_dir::TempDir::new(&format!("native-descriptors-{streams:?}"));
    let path = directory.0.clone();
    let observed = invoke(
        &["count", "1"],
        b"a\nb\n",
        streams,
        &path,
        stdout.len() + 1,
        stderr.len() + 1,
    );
    drop(directory);
    assert!(
        !path.try_exists().unwrap(),
        "{streams:?}: temporary cleanup"
    );
    let observed = observed.unwrap_or_else(|error| panic!("{streams:?}: {error}"));
    eprintln!(
        "{streams:?}: access flags={:?}, reaped {observed:?}",
        observed.parent_binding_flags
    );
    assert!(
        observed.parent_binding_fds.iter().all(|fd| *fd >= 3),
        "{streams:?}: {observed:?}"
    );
    assert!(
        observed
            .parent_close_results
            .iter()
            .all(|result| result.is_ok()),
        "{streams:?}: {observed:?}"
    );
    assert!(!observed.timed_out, "{streams:?}: {observed:?}");
    assert!(observed.cancellation.is_none(), "{streams:?}: {observed:?}");
    assert!(
        matches!(observed.status, WaitStatus::Exited(_, code) if code == status),
        "{streams:?}: {observed:?}"
    );
    assert_eq!(observed.stdout_length, stdout.len() as u64, "{streams:?}");
    assert_eq!(observed.stderr_length, stderr.len() as u64, "{streams:?}");
    assert_eq!(observed.stdout, stdout, "{streams:?}");
    assert_eq!(observed.stderr, stderr, "{streams:?}");
}

#[test]
fn concurrent_closed_stdin_invocations_keep_separate_captures_and_cleanup() {
    let start = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let start = &start;
            scope.spawn(move || {
                start.wait();
                let output = descriptor_process::closed_stdin(&["count", "1"]);
                assert_eq!(output.status.code(), Some(1), "{output:?}");
                assert!(output.stdout.is_empty(), "{output:?}");
                assert_eq!(
                    output.stderr,
                    b"fastmash: read error: Bad file descriptor\n"
                );
            });
        }
    });
}
