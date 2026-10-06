use super::*;

pub(super) fn failing_input(binary: &OsStr, arguments: &[&str], full: bool) -> Output {
    failing_input_with(binary, arguments, b"a\tb\nc\td\npartial", full, "C")
}

/// Runs `binary` on `input` from a terminal whose reads fail with EIO after it,
/// discarding a final incomplete record.
pub(super) fn failing_input_with(
    binary: &OsStr,
    arguments: &[&str],
    input: &[u8],
    full: bool,
    locale: &str,
) -> Output {
    use std::{io::Write, os::fd::FromRawFd};
    let mut master = -1;
    let mut slave = -1;
    // Successful openpty transfers two owned descriptors. Raw mode prevents
    // terminal newline conversion; closing the slave gives the master EIO after
    // its queued bytes, including one incomplete record, have been consumed.
    // SAFETY: every pointer is to a live local or null where the API allows it.
    unsafe {
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null()
            ),
            0
        );
        let mut term = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut term), 0);
        libc::cfmakeraw(&mut term);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &term), 0);
    }
    // SAFETY: openpty returned two new descriptors that nothing else owns.
    let terminal = unsafe { fs::File::from_raw_fd(master) };
    // SAFETY: as above.
    let mut feed = unsafe { fs::File::from_raw_fd(slave) };
    feed.write_all(input).unwrap();
    drop(feed);
    Command::new(binary)
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", locale)
        .env("PATH", "/usr/bin:/bin")
        .stdin(terminal)
        .stdout(if full {
            Stdio::from(
                fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn transpose_read_error_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    for full in [false, true] {
        let expected = failing_input(&reference, &["transpose"], full);
        fs::write(
            evidence.join(format!("gnu-{full}.txt")),
            format!("{expected:?}"),
        )
        .unwrap();
        assert_eq!(expected.status.code(), Some(1));
        assert_eq!(
            expected.stdout,
            if full {
                b"".as_slice()
            } else {
                b"a\tc\nb\td\n"
            }
        );
        let actual = failing_input(&candidate(), &["transpose"], full);
        fs::write(
            evidence.join(format!("candidate-{full}.txt")),
            format!("{actual:?}"),
        )
        .unwrap();
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.status, expected.status);
    }
}

#[test]
fn transpose_known_tables() {
    for (a, input, expected) in [
        (
            vec!["transpose"],
            b"a\tb\nc\td\n".as_slice(),
            b"a\tc\nb\td\n".as_slice(),
        ),
        (vec!["-H", "transpose"], b"a\tb\nc\td\n", b"a\tc\nb\td\n"),
        (
            vec!["--no-strict", "-F?", "transpose"],
            b"a\tb\nc\n",
            b"a\tc\nb\t?\n",
        ),
        (
            vec!["--vnlog", "transpose"],
            b"# a b\n1 2 #tail\n3 4\n",
            b"1 3\n2 4\n",
        ),
        (
            vec!["-z", "transpose"],
            b"a\nb\tc\0d\te\0",
            b"a\nb\td\0c\te\0",
        ),
    ] {
        let o = invoke(&candidate(), &args(&a), input, false, false);
        assert!(o.status.success(), "{a:?}: {o:?}");
        assert_eq!(o.stdout, expected, "{a:?}");
        assert!(o.stderr.is_empty());
    }
    let o = invoke(
        &candidate(),
        &args(&["transpose"]),
        b"a\tb\nc\n",
        false,
        false,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    assert_eq!(o.stderr,b"fastmash: transpose input error: line 2 has 1 fields (previous lines had 2);\nsee --help to disable strict mode\n");
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn transpose_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for flags in [
        vec![],
        vec!["-H"],
        vec!["--header-in"],
        vec!["--header-out"],
        vec!["-s"],
        vec!["-i"],
        vec!["-f"],
        vec!["--no-strict"],
        vec!["--no-strict", "-F?"],
        vec!["--no-strict", "--filler="],
        vec!["--no-strict", "-Fone", "-Ftwo"],
        vec!["-W"],
        vec!["-W", "--no-strict"],
        vec!["-t,"],
        vec!["-t,", "--no-strict", "--output-delimiter=|"],
        vec!["-C"],
        vec!["-C", "--no-strict"],
        vec!["--narm"],
        vec!["--format=%.2f"],
        vec!["--vnlog"],
        vec!["--vnlog", "--no-strict"],
        vec!["-F?", "--vnlog"],
        vec!["--vnlog", "-F?"],
        vec!["--vnlog", "--output-delimiter=:"],
    ] {
        for raw in [
            b"".as_slice(),
            b"\n",
            b"\n\n",
            b"a",
            b"a\tb\nc\td\n",
            b"a\tb\nc\n",
            b"a\nb\tc\n",
            b"\na\tb\n",
            b"a\tb\n\n",
            b"a\t\n\tb\n",
            b"a\0x\tb\nc\td\0y\n",
            b"\xff\t\xfe\n\x80\t\0\n",
            b" a  b  \n c d\n",
            b"a,b\nc,d\n",
            b"# key value\na 1 # tail\nb 2\n",
            b";comment\na\tb\n",
            b"a\r\nb\r\n",
        ] {
            for zero in [false, true] {
                let mut a = args(&flags);
                let mut input = raw.to_vec();
                if zero {
                    a.push("-z".into());
                    for b in &mut input {
                        if *b == b'\n' {
                            *b = 0;
                        }
                    }
                }
                a.push("transpose".into());
                cases.push((a, input));
            }
        }
    }
    for a in [
        vec!["TRANSPOSE"],
        vec!["transpose", "1"],
        vec!["transpose", "count", "1"],
        vec!["-g1", "transpose"],
        vec!["--filler", "transpose"],
        vec!["--vnlog", "-F-", "transpose"],
    ] {
        cases.push((args(&a), b"a\tb\nc\td\n".to_vec()));
    }
    let mut raw_filler = args(&["--no-strict", "transpose"]);
    raw_filler.push(OsString::from_vec(vec![b'-', b'F', 0xff, b'\n']));
    cases.push((raw_filler, b"a\tb\nc\n".to_vec()));
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn transpose_tall_wide_and_output_failure() {
    for (rows, cols) in [(100_000, 2), (2, 50_000), (1000, 1000)] {
        let mut input = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if c > 0 {
                    input.push(b'\t');
                }
                input.extend_from_slice(format!("{r}:{c}").as_bytes());
            }
            input.push(b'\n');
        }
        let o = invoke(&candidate(), &args(&["transpose"]), &input, false, false);
        assert!(o.status.success(), "{rows}x{cols}: {o:?}");
        let mut expected = Vec::new();
        for c in 0..cols {
            for r in 0..rows {
                if r > 0 {
                    expected.push(b'\t');
                }
                expected.extend_from_slice(format!("{r}:{c}").as_bytes());
            }
            expected.push(b'\n');
        }
        assert_eq!(o.stdout, expected);
    }
    let o = invoke(
        &candidate(),
        &args(&["transpose"]),
        b"a\tb\nc\td\n",
        false,
        true,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("No space left on device"));
}

#[test]
fn transpose_allocation_failure_is_explicit_and_has_no_output() {
    let path = std::env::temp_dir().join(format!("transpose-memory-{}", std::process::id()));
    // Three million tiny cells need more offset storage than this child's allowance.
    let input = b"x\tx\tx\tx\tx\tx\tx\tx\tx\tx\n".repeat(300_000);
    fs::write(&path, &input).unwrap();
    let mut command = Command::new(candidate());
    command
        .arg("transpose")
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The child changes only its own address-space limit before exec.
    // SAFETY: only async-signal-safe setrlimit runs between fork and exec.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 32 * 1024 * 1024,
                rlim_max: 32 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let o = command.output().unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(o.status.code(), Some(77), "{o:?}");
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).contains("transpose memory allocation failed"));
}

#[test]
#[ignore = "requires installed binary, named reference, RefGene and fresh evidence directory"]
fn transpose_measurements() {
    let time_binary =
        std::env::var_os("FASTMASH_TIME_BINARY").unwrap_or_else(|| "/usr/bin/time".into());
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let real = std::path::PathBuf::from(std::env::var_os("FASTMASH_REFGENE").unwrap());
    let small = evidence.join("tiny-cells.tsv");
    fs::write(&small, b"x\tx\tx\tx\tx\tx\tx\tx\tx\tx\n".repeat(100_000)).unwrap();
    let ragged = evidence.join("ragged.tsv");
    let mut input = b"x\n".repeat(999);
    input.extend_from_slice(&b"x\t".repeat(999));
    input.extend_from_slice(b"x\n");
    fs::write(&ragged, input).unwrap();
    for (job, input, arguments) in [
        ("refgene", real, args(&["transpose"])),
        ("tiny-cells", small, args(&["transpose"])),
        ("ragged", ragged, args(&["--no-strict", "transpose"])),
    ] {
        if job != "refgene" {
            let correctness = evidence.join(format!("{job}-correctness"));
            fs::create_dir(&correctness).unwrap();
            compare_cases(
                &[(arguments.clone(), fs::read(&input).unwrap())],
                &reference,
                &correctness,
            );
        }
        for round in 0..6 {
            let binaries = if round % 2 == 0 {
                vec![("gnu", reference.clone()), ("candidate", candidate())]
            } else {
                vec![("candidate", candidate()), ("gnu", reference.clone())]
            };
            for (label, binary) in binaries {
                let start = std::time::Instant::now();
                let output = Command::new(&time_binary)
                    .args(["-f", "%e %U %S %M", "-o"])
                    .arg(evidence.join(format!("{job}-{round}-{label}.time")))
                    .arg(binary)
                    .args(&arguments)
                    .env_clear()
                    .env("LC_ALL", "C")
                    .stdin(fs::File::open(&input).unwrap())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                    .unwrap();
                fs::write(
                    evidence.join(format!("{job}-{round}-{label}.elapsed-ns")),
                    start.elapsed().as_nanos().to_string(),
                )
                .unwrap();
                assert!(output.status.success(), "{job} {label}: {output:?}");
                assert!(output.stderr.is_empty());
            }
        }
    }
}

#[test]
#[ignore = "requires retained RefGene, named reference and fresh evidence directory"]
fn transpose_real_data() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let input = fs::read(std::env::var_os("FASTMASH_REFGENE").unwrap()).unwrap();
    compare_cases(&[(args(&["transpose"]), input)], &reference, &evidence);
}
