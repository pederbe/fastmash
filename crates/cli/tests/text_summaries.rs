use std::{
    fs::File,
    io::Write,
    process::{Command, Output, Stdio},
};

fn command(args: &[&str], input: &[u8]) -> Output {
    command_with_resources(args, input, false, false)
}

fn command_with_resources(args: &[&str], input: &[u8], full: bool, limited: bool) -> Output {
    let binary = std::env::var_os("FASTMASH_TEXT_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    // A file avoids pipe deadlock when wide or grouped output exceeds pipe capacity.
    let path = std::env::temp_dir().join(format!(
        "fastmash-text-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    File::create(&path).unwrap().write_all(input).unwrap();
    let mut child = Command::new("/usr/bin/timeout");
    child.args(["--kill-after=2s", "30s"]);
    if limited {
        child.args(["/usr/bin/prlimit", "--as=33554432", "--"]);
    }
    let out = child
        .arg(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(File::open(&path).unwrap())
        .stdout(if full {
            Stdio::from(File::options().write(true).open("/dev/full").unwrap())
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    out
}

#[test]
fn text_collections_accept_more_than_the_old_value_limit() {
    let input = b"transcript\n".repeat(70_000);
    let out = command(
        &[
            "count",
            "1",
            "first",
            "1",
            "last",
            "1",
            "unique",
            "1",
            "countunique",
            "1",
            "collapse",
            "1",
        ],
        &input,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut expected = b"70000\ttranscript\ttranscript\ttranscript\t1\t".to_vec();
    expected.extend(b"transcript,".repeat(69_999));
    expected.extend_from_slice(b"transcript\n");
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
}

#[test]
fn wide_text_and_collections_exceed_old_byte_limits() {
    let value = "x".repeat(9000);
    let input = format!("{value}\n").repeat(1000);
    let out = command(
        &[
            "count",
            "1",
            "first",
            "1",
            "last",
            "1",
            "unique",
            "1",
            "countunique",
            "1",
            "collapse",
            "1",
        ],
        input.as_bytes(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut expected = format!("1000\t{value}\t{value}\t{value}\t1\t");
    expected.push_str(&format!("{value},").repeat(999));
    expected.push_str(&value);
    expected.push('\n');
    assert_eq!(out.stdout, expected.as_bytes());
    assert!(out.stderr.is_empty());
}

#[test]
fn text_summaries_preserve_byte_order_nuls_and_missing_values() {
    let args = [
        "count",
        "1",
        "first",
        "1",
        "last",
        "1",
        "unique",
        "1",
        "countunique",
        "1",
        "collapse",
        "1",
    ];
    for (input, expected) in [
        (b"".as_slice(), b"".as_slice()),
        (b"b\na\nb", b"3\tb\tb\ta,b\t2\tb,a,b\n"),
        (b"b\0\0a\nz\ny\n", b"3\tb\ty\t,a,b\t3\tb,,a,z,y\n"),
        (
            b"\xff\nA\na\n\tunused\nA\n",
            b"5\t\xff\tA\t,A,a,\xff\t4\t\xff,A,a,,A\n",
        ),
        (b"\tunused\n", b"1\t\t\t\t1\t\n"),
    ] {
        let out = command(&args, input);
        assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
        assert_eq!(out.stdout, expected, "{input:?}");
        assert!(out.stderr.is_empty());
    }
    let out = command(
        &[
            "--narm",
            "count",
            "1",
            "first",
            "1",
            "last",
            "1",
            "unique",
            "1",
            "countunique",
            "1",
            "collapse",
            "1",
        ],
        b"NA\nNaN\nN/A\n",
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"0\tN/A\tN/A\t\t0\t\n");
}

#[test]
fn wide_records_allow_text_and_decimal_outputs_together() {
    let input = format!(
        "1.25\tz\t{}\n2.75\ta\t{}\n",
        "x".repeat(9000),
        "y".repeat(9000)
    );
    let out = command(
        &[
            "--output-delimiter=|",
            "-c",
            ":",
            "count",
            "2",
            "first",
            "2",
            "last",
            "2",
            "unique",
            "2",
            "countunique",
            "2",
            "collapse",
            "2",
            "sum",
            "1",
            "mean",
            "1",
        ],
        input.as_bytes(),
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"2|z|a|a:z|2|z:a|4|2\n");
    assert!(out.stderr.is_empty());
}

#[test]
#[ignore = "requires installed binary and sorting companion"]
fn installed_sorted_wide_headers_keys_and_group_resets() {
    let key = "k".repeat(9000);
    let input = format!(
        "#{}\nkey\tvalue\t{}\n{key}\tb\tx\na\tz\tx\n{key}\ta\tx\n",
        "comment".repeat(1400),
        "unused".repeat(1500)
    );
    let out = command(
        &[
            "-C",
            "--header-in",
            "-s",
            "-g",
            "key",
            "first",
            "value",
            "last",
            "value",
            "unique",
            "value",
            "countunique",
            "value",
            "collapse",
            "value",
        ],
        input.as_bytes(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        format!("a\tz\tz\tz\t1\tz\n{key}\tb\ta\ta,b\t2\tb,a\n").as_bytes()
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn text_output_failure_and_later_group_error_remain_visible() {
    let out = command_with_resources(
        &["collapse", "1"],
        &b"transcript\n".repeat(70_000),
        true,
        false,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
    let out = command(&["-g", "1", "collapse", "2"], b"a\tx\na\ty\nb\n");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"a\tx,y\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("field"));
}

#[test]
fn text_allocation_failure_is_explicit() {
    let out = command_with_resources(
        &["collapse", "1"],
        &b"transcript\n".repeat(4_000_000),
        false,
        true,
    );
    assert_eq!(out.status.code(), Some(77), "{:?}", out.stderr);
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("text memory allocation failed"));
}

#[test]
fn text_final_output_allocation_failure_is_explicit() {
    let out = command_with_resources(
        &["collapse", "1"],
        &b"transcript\n".repeat(1_000_000),
        false,
        true,
    );
    assert_eq!(out.status.code(), Some(77), "{:?}", out.stderr);
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("output memory allocation failed"));
}
