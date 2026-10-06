#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;
#[cfg(target_os = "macos")]
#[path = "support/temp_dir.rs"]
mod temp_dir;

use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::{ffi::OsStringExt, process::CommandExt},
    process::{Command, Output, Stdio},
};

#[path = "record_separators/reproducibility.rs"]
mod reproducibility;

#[path = "record_separators/random_jobs.rs"]
mod random_jobs;

#[path = "record_separators/base64_jobs.rs"]
mod base64_jobs;

#[path = "../src/decimal_fixtures.rs"]
mod decimal_fixtures;

#[path = "record_separators/percentile_trimmean.rs"]
mod percentile_trimmean;

#[path = "record_separators/robust.rs"]
mod robust;

#[path = "record_separators/crosstab.rs"]
mod crosstab;

#[path = "record_separators/line_numeric.rs"]
mod line_numeric;

#[path = "record_separators/binning.rs"]
mod binning;

#[path = "record_separators/grouped_lookup.rs"]
mod grouped_lookup;

#[path = "record_separators/fractional_decimal.rs"]
mod fractional_decimal;

#[path = "record_separators/numeric_input.rs"]
mod numeric_input;

#[path = "record_separators/locale_migration.rs"]
mod locale_migration;

#[path = "record_separators/locale_delivery.rs"]
mod locale_delivery;

#[path = "record_separators/alternative_means.rs"]
mod alternative_means;

#[path = "record_separators/paired_jobs.rs"]
mod paired_jobs;

#[path = "record_separators/malformed_input.rs"]
mod malformed_input;

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_RECORD_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn invoke(binary: &OsStr, arguments: &[OsString], input: &[u8], spill: bool, full: bool) -> Output {
    invoke_locale(binary, arguments, input, spill, full, "C")
}

fn invoke_locale(
    binary: &OsStr,
    arguments: &[OsString],
    input: &[u8],
    spill: bool,
    full: bool,
    locale: &str,
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "records-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LANG", "C")
        .env("LC_NUMERIC", locale)
        .env("PATH", "/usr/bin:/bin")
        .stdin(fs::File::open(&path).unwrap())
        .full_stdout(full)
        .stderr(Stdio::piped());
    if spill {
        command.env("FASTMASH_SORT_MEMORY_BYTES", "1");
    }
    if locale != "C" {
        command.env(
            "LOCPATH",
            std::env::var_os("FASTMASH_RECORD_LOCALE_PATH")
                .expect("named GNU locale directory required"),
        );
    }
    let output = command.output().unwrap();
    fs::remove_file(path).unwrap();
    output
}

#[test]
fn nul_records_keep_newlines_and_terminate_headers_and_results() {
    for (a, input, expected) in [
        (
            args(&["-z", "collapse", "1"]),
            b"a\nb\0c\0".as_slice(),
            b"a\nb,c\0".as_slice(),
        ),
        (
            args(&["-zH", "sum", "value"]),
            b"value\0".as_slice(),
            b"sum(value)\0".as_slice(),
        ),
        (
            args(&["-zH", "sum", "value"]),
            b"value\x001\x002".as_slice(),
            b"sum(value)\x003\0".as_slice(),
        ),
        (
            args(&["-zsg1", "sum", "2"]),
            b"b\t2\0a\t1\0a\t3\0".as_slice(),
            b"a\t4\0b\t2\0".as_slice(),
        ),
    ] {
        let output = invoke(&candidate(), &a, input, false, false);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn long_raw_records_survive_native_spill_and_output_failure() {
    let mut value = vec![0xff; 30_000];
    value[15_000] = b'\n';
    let mut input = b"b\t".to_vec();
    input.extend_from_slice(&value);
    input.extend_from_slice(b"\0a\tx\0");
    let mut expected = b"a\tx\0b\t".to_vec();
    expected.extend_from_slice(&value);
    expected.push(0);
    let a = args(&["-zsg1", "first", "2"]);
    let output = invoke(&candidate(), &a, &input, true, false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, expected);
    let output = invoke(&candidate(), &a, &input, true, true);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("No space left on device"));
}

#[test]
fn native_original_spill_and_external_header_handoff() {
    for (a, input, expected, spill) in [
        (
            args(&["-zsg1", "sum", "2", "first", "3", "last", "3"]),
            b"b\nkey\t2\tx\0a\t1\tfirst\nvalue\0a\t3\tlast\nvalue".as_slice(),
            b"a\t4\tfirst\nvalue\tlast\nvalue\0b\nkey\t2\tx\tx\0".as_slice(),
            true,
        ),
        (
            args(&["-zHsg1", "pcov", "2:3"]),
            b"key\nname\tx\ty\0b\t2\t4\0a\t1\t2\0a\t3\t6".as_slice(),
            b"GroupBy(key\nname)\tpcov(x,y)\0a\t2\0b\t0\0".as_slice(),
            false,
        ),
    ] {
        let output = invoke(&candidate(), &a, input, spill, false);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn named_reference_record_separators() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for a in [
        args(&["-zHsg1", "pcov", "2:3"]),
        args(&["-zHsg1", "sum", "2"]),
    ] {
        cases.push((a, b"key\nname\tx\ty\0b\t2\t4\0a\t1\t2\0a\t3\t6".to_vec()));
    }
    for flags in [
        vec!["-zC"],
        vec!["-zCH"],
        vec!["-zCsHg1"],
        vec!["-z", "--header-out"],
    ] {
        let mut a = args(&flags);
        a.extend(args(&["count", "1"]));
        cases.push((a, b"# comment\ncontinued\0key\0b\0a\0a\0".to_vec()));
    }
    cases.push((args(&["-z", "count", "2"]), b"a\tb\0c\nembedded\0".to_vec()));
    for zero in [false, true] {
        for input in [
            b"".as_slice(),
            b"\n",
            b"\0",
            b"\0\0",
            b"a",
            b"a\n",
            b"a\0",
            b"a\r\nb\r\n",
            b"a\0tail\nb\0end\n",
            b"a\nb\0c\0",
            b"\xff\t\x80\n\0",
            b"\t\t\n",
            b" \t \n",
            b" a\t b \t\n",
            b"a\x0b\x0c\rb\n",
        ] {
            for mode in [None, Some("-W"), Some("-t,")] {
                for operation in [
                    "count",
                    "first",
                    "last",
                    "unique",
                    "countunique",
                    "collapse",
                ] {
                    let mut a = Vec::new();
                    if zero {
                        a.push("-z".into());
                    }
                    if let Some(mode) = mode {
                        a.push(mode.into());
                    }
                    a.extend(args(&[operation, "1"]));
                    cases.push((a, input.to_vec()));
                }
            }
        }
        let end = if zero { 0 } else { b'\n' };
        for flags in [
            vec![],
            vec!["-g1"],
            vec!["-sg1"],
            vec!["-H"],
            vec!["-Hg1"],
            vec!["-Hsg1"],
            vec!["--header-out", "-sg1"],
        ] {
            for operation in ["sum", "first", "collapse", "pcov"] {
                let mut a = args(&flags);
                if zero {
                    a.push("--zero-terminated".into());
                }
                a.extend(args(&[
                    operation,
                    if operation == "pcov" { "2:3" } else { "2" },
                ]));
                let mut input = Vec::new();
                if flags.iter().any(|f| f.starts_with("-H")) {
                    input.extend_from_slice(b"key\tvalue\tother");
                    input.push(end);
                }
                for row in [b"b\t2\t4".as_slice(), b"a\t1\t2", b"a\t3\t6"] {
                    input.extend_from_slice(row);
                    input.push(end);
                }
                cases.push((a, input));
            }
        }
        for delimiter in [b',', b' ', b'\t', b'\n', b'\r', 0x80, 0xff] {
            for options in [
                vec!["-t", "--output-delimiter", "-c"],
                vec!["--output-delimiter", "-t", "-W"],
                vec!["-W", "-t"],
            ] {
                let mut a = Vec::new();
                if zero {
                    a.push("-z".into());
                }
                for option in options {
                    a.push(option.into());
                    if option != "-W" {
                        a.push(OsString::from_vec(vec![delimiter]));
                    }
                }
                a.extend(args(&["collapse", "1", "first", "2"]));
                cases.push((
                    a,
                    vec![b'a', delimiter, b'b', end, b'c', delimiter, b'd', end],
                ));
            }
        }
        for option in ["-t", "-c", "--output-delimiter"] {
            for value in ["", "xx", "é"] {
                let mut a = args(&[option, value, "count", "1"]);
                if zero {
                    a.insert(0, "-z".into());
                }
                cases.push((a, vec![]));
            }
        }
        for length in [8191, 8192, 8193, 100_000] {
            let mut input = vec![0xff; length];
            input.push(end);
            let mut a = args(&["--header-out", "first", "1", "count", "1"]);
            if zero {
                a.insert(0, "-z".into());
            }
            cases.push((a, input));
        }
    }
    compare_cases(&cases, &reference, &evidence);
}

fn compare_cases(
    cases: &[(Vec<OsString>, Vec<u8>)],
    reference: &OsStr,
    evidence: &std::path::Path,
) {
    compare_cases_locale(cases, reference, evidence, "C");
}

fn compare_cases_locale(
    cases: &[(Vec<OsString>, Vec<u8>)],
    reference: &OsStr,
    evidence: &std::path::Path,
    locale: &str,
) {
    compare_cases_settings(cases, reference, evidence, locale, false);
}

fn compare_cases_settings(
    cases: &[(Vec<OsString>, Vec<u8>)],
    reference: &OsStr,
    evidence: &std::path::Path,
    locale: &str,
    spill: bool,
) {
    fs::write(evidence.join("numeric-locale.txt"), locale).unwrap();
    fs::write(evidence.join("force-spill.txt"), spill.to_string()).unwrap();
    let mut failures = Vec::new();
    for (index, (a, input)) in cases.iter().enumerate() {
        fs::write(evidence.join(format!("{index:04}.args")), format!("{a:?}")).unwrap();
        fs::write(evidence.join(format!("{index:04}.input")), input).unwrap();
        let expected = invoke_locale(reference, a, input, false, false, locale);
        let actual = invoke_locale(&candidate(), a, input, spill, false, locale);
        for (label, output) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(
                evidence.join(format!("{index:04}.{label}.stdout")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{index:04}.{label}.stderr")),
                &output.stderr,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{index:04}.{label}.status")),
                output.status.to_string(),
            )
            .unwrap();
        }
        if actual.status != expected.status
            || actual.stdout != expected.stdout
            || actual.stderr != expected.stderr
        {
            failures.push(index);
        }
    }
    eprintln!("{} comparisons, failures: {failures:?}", cases.len());
    assert!(failures.is_empty());
}

#[path = "record_separators/full_rows.rs"]
mod full_rows;

#[path = "record_separators/annotated.rs"]
mod annotated;

#[path = "record_separators/field_modes.rs"]
mod field_modes;

#[path = "record_separators/table_checks.rs"]
mod table_checks;

#[path = "record_separators/case_aware.rs"]
mod case_aware;

#[path = "record_separators/record_intake.rs"]
mod record_intake;

#[path = "record_separators/transpose.rs"]
mod transpose;

#[path = "record_separators/moments_jobs.rs"]
mod moments_jobs;

#[path = "record_separators/normality_jobs.rs"]
mod normality_jobs;

#[path = "record_separators/checksum_jobs.rs"]
mod checksum_jobs;

#[path = "record_separators/path_jobs.rs"]
mod path_jobs;

#[path = "record_separators/enhancement_decision.rs"]
mod enhancement_decision;
