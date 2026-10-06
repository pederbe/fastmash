use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn crosstab_read_failures() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    for full in [false, true] {
        let expected = super::transpose::failing_input(&reference, &["ct", "1,2"], full);
        let actual = super::transpose::failing_input(&candidate(), &["ct", "1,2"], full);
        fs::write(
            evidence.join(format!("gnu-{full}.txt")),
            format!("{expected:?}"),
        )
        .unwrap();
        fs::write(
            evidence.join(format!("candidate-{full}.txt")),
            format!("{actual:?}"),
        )
        .unwrap();
        assert_eq!(expected.status.code(), Some(1));
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.status, expected.status);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn crosstab_allocation_failure_is_explicit() {
    let path = std::env::temp_dir().join(format!("crosstab-memory-{}", std::process::id()));
    let mut input = Vec::new();
    for r in 0..500_000 {
        input.extend_from_slice(format!("{r}\tx\n").as_bytes());
    }
    fs::write(&path, input).unwrap();
    let mut command = Command::new(candidate());
    command
        .args(["ct", "1,2"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Restrict only the child address space; table growth must return a diagnostic.
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
    let output = command.output().unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(output.status.code(), Some(77), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("crosstab memory allocation failed"));
}

#[test]
#[ignore = "requires installed candidate, GNU reference, RefGene and fresh evidence directory"]
fn crosstab_measurements() {
    let time = std::env::var_os("FASTMASH_TIME_BINARY").unwrap_or_else(|| "/usr/bin/time".into());
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let real = std::path::PathBuf::from(std::env::var_os("FASTMASH_REFGENE").unwrap());
    let dense = evidence.join("dense.tsv");
    let sparse = evidence.join("sparse.tsv");
    let mut data = Vec::new();
    for r in 0..500 {
        for c in 0..500 {
            data.extend_from_slice(format!("r{r:04}\tc{c:04}\n").as_bytes());
        }
    }
    fs::write(&dense, data).unwrap();
    let mut data = Vec::new();
    for r in 0..1000 {
        data.extend_from_slice(format!("r{r:04}\tc{r:04}\n").as_bytes());
    }
    fs::write(&sparse, data).unwrap();
    for (name, input, a) in [
        ("refgene", real, args(&["-s", "ct", "3,4"])),
        ("dense", dense, args(&["ct", "1,2"])),
        ("sparse", sparse, args(&["ct", "1,2"])),
    ] {
        let correctness = evidence.join(format!("{name}-correctness"));
        fs::create_dir(&correctness).unwrap();
        compare_cases(
            &[(a.clone(), fs::read(&input).unwrap())],
            &reference,
            &correctness,
        );
        for round in 0..6 {
            let mut binaries = vec![("gnu", reference.clone()), ("candidate", candidate())];
            if round % 2 == 1 {
                binaries.reverse();
            }
            for (label, binary) in binaries {
                let start = std::time::Instant::now();
                let o = Command::new(&time)
                    .args(["-f", "%e %U %S %M", "-o"])
                    .arg(evidence.join(format!("{name}-{round}-{label}.time")))
                    .arg(binary)
                    .args(&a)
                    .env_clear()
                    .env("LC_ALL", "C")
                    .env("PATH", "/usr/bin:/bin")
                    .stdin(fs::File::open(&input).unwrap())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                    .unwrap();
                fs::write(
                    evidence.join(format!("{name}-{round}-{label}.elapsed-ns")),
                    start.elapsed().as_nanos().to_string(),
                )
                .unwrap();
                assert!(o.status.success(), "{name} {label}: {o:?}");
                assert!(o.stderr.is_empty());
            }
        }
    }
}

#[test]
fn crosstab_retains_long_labels_and_first_completed_cells() {
    for sorted in [false, true] {
        let prefix = "a".repeat(20_000);
        let input = format!("{prefix}X\tx\t1\n{prefix}Y\tx\t2\n{prefix}X\tx\t3\n");
        let mut a = args(&["ct", "1,2", "sum", "3"]);
        if sorted {
            a.insert(0, "-s".into());
        }
        let o = invoke(&candidate(), &a, input.as_bytes(), false, false);
        assert!(o.status.success(), "{o:?}");
        assert_eq!(
            o.stdout,
            format!(
                "\tx\n{prefix}X\t{}\n{prefix}Y\t2\n",
                if sorted { 4 } else { 1 }
            )
            .as_bytes()
        );
    }
    let prefix = "c".repeat(20_000);
    let input = format!("r\t{prefix}X\nr\t{prefix}Y\n");
    let o = invoke(
        &candidate(),
        &args(&["ct", "1,2"]),
        input.as_bytes(),
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        o.stdout,
        format!("\t{prefix}X\t{prefix}Y\nr\t1\t1\n").as_bytes()
    );
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn crosstab_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for flags in [
        vec![],
        vec!["-s"],
        vec!["-i"],
        vec!["-si"],
        vec!["-H"],
        vec!["--header-in"],
        vec!["--header-out"],
        vec!["-sH"],
        vec!["-fH"],
        vec!["-sfH"],
        vec!["-f"],
        vec!["--filler=?"],
        vec!["--filler="],
        vec!["-W"],
        vec!["-t,"],
        vec!["--output-delimiter=|"],
        vec!["-C"],
        vec!["--narm"],
        vec!["--no-strict"],
        vec!["--vnlog"],
    ] {
        for operation in [
            vec![],
            vec!["count", "3"],
            vec!["first", "3"],
            vec!["last", "3"],
            vec!["sum", "3"],
            vec!["unique", "3"],
        ] {
            for input in [
                b"".as_slice(),
                b"\n",
                b"r\tc\tv\n",
                b"b\ty\t2\na\tx\t1\na\tx\t3\nb\ty\t4\n",
                b"a\tx\t1\nA\tx\t2\nb\ty\t3\nA\tx\t4\n",
                b"a\0one\tx\t1\na\0two\tx\t2\na\0long\tx\t3\n",
                b"\t\t\n",
                b"a\tx\t1\nb\ty\tbad\n",
                b"# r c v\na x 1 #note\nb y 2\n",
                b"a,x,1\nb,y,2\n",
                b"a\tx\t1\nb\n",
            ] {
                let mut a = flags.clone();
                a.extend(["crosstab", "1,2"]);
                a.extend(operation.iter().copied());
                cases.push((args(&a), input.to_vec()));
            }
        }
    }
    for a in [
        vec!["ct"],
        vec!["ct", "1"],
        vec!["ct", "1,2,3"],
        vec!["ct", "1:2"],
        vec!["ct", "1-2"],
        vec!["ct", "2,1"],
        vec!["ct", "1,1"],
        vec!["ct", "1,2", "sum", "3", "count", "3"],
        vec!["ct", "1,2", "sum", "2,3"],
        vec!["ct", "1,2", "cut", "3"],
        vec!["-g1", "ct", "1,2"],
        vec!["-H", "ct", "r,c", "sum", "v"],
        vec!["-sH", "ct", "r,c"],
        vec!["-z", "ct", "1,2"],
        vec!["--vnlog", "ct", "r,c"],
    ] {
        cases.push((args(&a), b"r\tc\tv\na\tx\t1\nb\ty\t2\n".to_vec()));
    }
    cases.push((args(&["-z", "ct", "1,2"]), b"a\tx\0b\ty\0a\tx\0".to_vec()));
    cases.push((
        args(&["ct", "1,2", "first", "3"]),
        b"\xff\t\xfe\tv\0after\n\x80\tx\tv\n".to_vec(),
    ));
    for op in [
        "mean",
        "median",
        "min",
        "max",
        "countunique",
        "collapse",
        "range",
        "pstdev",
        "mode",
        "perc:50",
        "trimmean:0",
    ] {
        cases.push((
            args(&["-s", "ct", "1,2", op, "3"]),
            b"b\ty\t2\na\tx\t1\na\tx\t3\n".to_vec(),
        ));
    }
    for a in [
        vec!["-sH", "ct", "r,c"],
        vec!["-H", "ct", "r,c"],
        vec!["--format=%.2f", "ct", "1,2", "sum", "3"],
        vec!["-s", "ct", "1,2", "geomean", "3"],
        vec!["ct", "1,2", "pcov", "3:4"],
        vec!["ct", "1,2", "count", "1"],
        vec!["ct", "1,2", "groupby", "3", "sum", "4"],
    ] {
        for input in [b"".as_slice(), b"r\tc\tv\tw\n", b"a\tx\t1\t2\na\tx\t3\t4\n"] {
            cases.push((args(&a), input.to_vec()));
        }
    }
    compare_cases_settings(
        &cases,
        &reference,
        &evidence,
        "C",
        std::env::var_os("FASTMASH_CROSSTAB_SPILL").is_some(),
    );
}

#[test]
fn crosstab_cardinality_and_output_failure() {
    for (rows, columns) in [(100_000, 1), (1, 20_000), (500, 500)] {
        let mut input = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                input.extend_from_slice(format!("r{r:06}\tc{c:06}\n").as_bytes());
            }
        }
        let o = invoke(&candidate(), &args(&["ct", "1,2"]), &input, false, false);
        assert!(o.status.success(), "{rows}x{columns}: {o:?}");
        let mut expected = Vec::new();
        for c in 0..columns {
            expected.extend_from_slice(format!("\tc{c:06}").as_bytes());
        }
        expected.push(b'\n');
        for r in 0..rows {
            expected.extend_from_slice(format!("r{r:06}").as_bytes());
            for _ in 0..columns {
                expected.extend_from_slice(b"\t1");
            }
            expected.push(b'\n');
        }
        assert_eq!(o.stdout, expected);
    }
    let o = invoke(&candidate(), &args(&["ct", "1,2"]), b"a\tx\n", false, true);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        o.stderr
            .ends_with(b"write error: No space left on device\n")
    );
}
