#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;
#[path = "support/process.rs"]
#[cfg(target_os = "macos")]
mod process;
#[cfg(target_os = "linux")]
use std::process::Command;

use std::{
    fs,
    process::{Output, Stdio},
};

fn command(args: &[&str], input: &[u8]) -> Output {
    invoke(args, input, None, false)
}

fn invoke(args: &[&str], input: &[u8], address_space: Option<u64>, full: bool) -> Output {
    let binary = std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    let path = std::env::temp_dir().join(format!(
        "quantiles-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("/usr/bin/timeout");
        command.args(["--kill-after=2s", "60s"]);
        if let Some(bytes) = address_space {
            command
                .arg("/usr/bin/prlimit")
                .arg(format!("--as={bytes}"))
                .arg("--");
        }
        command.arg(&binary);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = process::bounded(&binary, 60);
    #[cfg(target_os = "macos")]
    if let Some(bytes) = address_space {
        panic!(
            "the Linux address-space fixture ({bytes} bytes) needs a native allocation-failure mechanism"
        );
    }
    let result = command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(fs::File::open(&path).unwrap())
        .full_stdout(full)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    result
}

#[test]
fn output_failure_is_reported() {
    let out = invoke(ALL, b"1\n3\n", None, true);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("write error"));
}

#[test]
#[ignore = "requires the installed release binary and Linux address-space limits"]
fn growth_and_sort_scratch_exhaustion_are_reported() {
    assert!(std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY").is_some());
    for (rows, diagnostic) in [
        (2_000_000, "sample memory allocation failed"),
        (1_000_000, "sample sorting memory allocation failed"),
    ] {
        let out = invoke(ALL, &b"1\n".repeat(rows), Some(28 * 1024 * 1024), false);
        println!(
            "rows={rows} address_space=29360128 status={:?} stdout={:?} stderr={:?}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(77), "{rows}: {:?}", out.stderr);
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(diagnostic),
            "{rows}: {:?}",
            out.stderr
        );
    }
}

#[test]
#[ignore = "requires the installed numerical sorting companion"]
fn prepared_sorted_headers_share_quantiles() {
    assert!(std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY").is_some());
    success(&["-H", "-s", "-g", "key", "median", "value", "q1", "2", "q3", "value", "iqr", "2"],
        b"key\tvalue\nb\t7\na\t3\na\t1\n",
        b"GroupBy(key)\tmedian(value)\tq1(value)\tq3(value)\tiqr(value)\na\t2\t1.5\t2.5\t1\nb\t7\t7\t7\t0\n");
}

#[test]
#[ignore = "requires installed release binary and Linux address-space limits"]
fn percentile_and_trim_allocation_failures() {
    assert!(std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY").is_some());
    for operation in ["perc:100", "trimmean:0.1"] {
        for (rows, diagnostic) in [
            (2_000_000, "sample memory allocation failed"),
            (1_000_000, "sample sorting memory allocation failed"),
        ] {
            let out = invoke(
                &[operation, "1"],
                &b"1\n".repeat(rows),
                Some(28 * 1024 * 1024),
                false,
            );
            println!(
                "{operation} rows={rows}: status={:?} stderr={:?}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.status.code(), Some(77));
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains(diagnostic));
        }
    }
}

const ALL: &[&str] = &["q1", "1", "median", "1", "q3", "1", "iqr", "1"];

#[test]
#[ignore = "requires installed release binary and Linux address-space limits"]
fn robust_allocation_failures() {
    assert!(std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY").is_some());
    for operation in ["mode", "antimode", "madraw", "mad"] {
        for (rows, diagnostic) in [
            (2_000_000, "sample memory allocation failed"),
            (1_000_000, "sample sorting memory allocation failed"),
        ] {
            let out = invoke(
                &[operation, "1"],
                &b"1\n".repeat(rows),
                Some(28 * 1024 * 1024),
                false,
            );
            println!(
                "{operation} rows={rows}: status={:?} stderr={:?}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.status.code(), Some(77));
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains(diagnostic));
        }
    }
}

fn success(args: &[&str], input: &[u8], expected: &[u8]) {
    let out = command(args, input);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected);
    assert!(out.stderr.is_empty());
}

#[test]
fn full_size_quantiles_cross_the_former_sample_limit() {
    use std::fmt::Write;
    let mut input = String::new();
    for n in (1..=100_000).rev() {
        writeln!(input, "{n}").unwrap();
    }
    success(
        ALL,
        input.as_bytes(),
        b"25000.75\t50000.5\t75000.25\t49999.5\n",
    );
}

#[test]
fn boundary_sizes_and_decimal_interpolation() {
    for (input, expected) in [
        ("", ""),
        ("1.25\n", "1.25\t1.25\t1.25\t0\n"),
        ("1\n3\n", "1.5\t2\t2.5\t1\n"),
        ("3\n1\n2\n", "1.5\t2\t2.5\t1\n"),
        ("4\n2\n1\n3\n", "1.75\t2.5\t3.25\t1.5\n"),
        ("5\n4\n3\n2\n1\n", "2\t3\t4\t2\n"),
        ("6\n5\n4\n3\n2\n1\n", "2.25\t3.5\t4.75\t2.5\n"),
        ("7\n6\n5\n4\n3\n2\n1\n", "2.5\t4\t5.5\t3\n"),
        ("8\n7\n6\n5\n4\n3\n2\n1\n", "2.75\t4.5\t6.25\t3.5\n"),
        ("0.25\n-0.75\n1.25\n", "-0.25\t0.25\t0.75\t1\n"),
    ] {
        success(ALL, input.as_bytes(), expected.as_bytes());
    }
}

#[test]
fn shared_named_fields_reset_and_other_statistics_stay_independent() {
    success(&["-H", "--narm", "-g", "group", "median", "value", "q1", "2", "q3", "value", "iqr", "2", "median", "2"],
        b"group\tvalue\na\t3\na\t1\nb\tNA\nb\tNaN\nc\t5\n",
        b"GroupBy(group)\tmedian(value)\tq1(value)\tq3(value)\tiqr(value)\tmedian(value)\na\t2\t1.5\t2.5\t1\t2\nb\tnan\tnan\tnan\tnan\tnan\nc\t5\t5\t5\t0\t5\n");
    success(
        &[
            "median", "1", "madraw", "1", "q1", "1", "pvar", "1", "q3", "1", "median", "2",
        ],
        b"9\t10\n1\t20\n5\t30\n",
        b"5\t4\t3\t10.666666666667\t7\t20\n",
    );
}

#[test]
fn request_order_does_not_resort_nan_samples() {
    for input in [
        b"3\nnan\n1\n2\n".as_slice(),
        b"-0\n0\n-0\n0\n",
        b"inf\n1\n-inf\n",
        b"-nan\n1\n2\n3\nnan\n",
    ] {
        let combined = command(
            &[
                "median", "1", "iqr", "1", "q3", "1", "q1", "1", "median", "1",
            ],
            input,
        );
        assert_eq!(combined.status.code(), Some(0), "{:?}", combined.stderr);
        let mut separate = Vec::new();
        for kind in ["median", "iqr", "q3", "q1", "median"] {
            let out = command(&[kind, "1"], input);
            assert_eq!(out.status.code(), Some(0));
            assert!(out.stderr.is_empty());
            separate.extend_from_slice(&out.stdout[..out.stdout.len() - 1]);
            separate.push(b'\t');
        }
        *separate.last_mut().unwrap() = b'\n';
        assert_eq!(combined.stdout, separate, "{input:?}");
        assert!(combined.stderr.is_empty());
    }
}

#[test]
fn long_records_and_invalid_fields_preserve_command_behavior() {
    success(
        ALL,
        format!("1\t{}\n3\t{}", "x".repeat(20000), "y".repeat(20000)).as_bytes(),
        b"1.5\t2\t2.5\t1\n",
    );
    for input in [
        b"1e5000\tbad\n".as_slice(),
        b"1e-5000\tbad\n",
        b"1 \tbad\n",
        b"1\0x\tbad\n",
    ] {
        let out = command(&["q1", "1", "median", "2", "iqr", "1"], input);
        assert_eq!(out.status.code(), Some(1), "{:?}", out.stderr);
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("line 1 field 1:"));
    }
    let out = command(&["q1", "1", "median", "2", "iqr", "1"], b"1\tbad\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 1 field 2:"));
}

#[test]
#[ignore = "requires installed release binary and Linux address-space limits"]
fn dedup_allocation_failure_and_duplicate_heavy_stream() {
    assert!(std::env::var_os("FASTMASH_QUANTILE_TEST_BINARY").is_some());
    let repeated = invoke(
        &["rmdup", "1"],
        &b"same\trow\n".repeat(500_000),
        Some(28 * 1024 * 1024),
        false,
    );
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(repeated.stdout, b"same\trow\n");
    let input: String = (0..600_000).map(|n| format!("key-{n}\n")).collect();
    let failed = invoke(
        &["rmdup", "1"],
        input.as_bytes(),
        Some(28 * 1024 * 1024),
        false,
    );
    assert_eq!(failed.status.code(), Some(77));
    assert!(failed.stdout.starts_with(b"key-0\n"));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("memory allocation failed"));
}
