use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn field_modes_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for command in [
        vec!["cut", "2,1,2"],
        vec!["echo", "1-2"],
        vec!["reverse"],
        vec!["nop"],
        vec!["noop"],
    ] {
        for flags in [
            vec![],
            vec!["-f"],
            vec!["-H"],
            vec!["--header-in"],
            vec!["--header-out"],
            vec!["--no-strict"],
            vec!["--filler=X"],
            vec!["-W"],
            vec!["-t,"],
            vec!["--output-delimiter=|"],
            vec!["-C"],
            vec!["--narm"],
            vec!["-s"],
        ] {
            for input in [
                b"".as_slice(),
                b"a\tb\nc\td\n",
                b"a\tb\nc\n",
                b"\n\n",
                b" a  b \n c\td",
                b"a,b\nc,d\n",
                b"a\0x\t\xffb\nc\td\n",
                b"# hi\na\tb\n",
                b"a\tb",
            ] {
                let mut a = args(&flags);
                a.extend(args(&command));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for a in [
        vec!["cut"],
        vec!["cut", "0"],
        vec!["cut", "2-1"],
        vec!["cut", "1:2"],
        vec!["cut", "1", "echo", "2"],
        vec!["cut", "1", "count", "2"],
        vec!["count", "1", "cut", "2"],
        vec!["reverse", "1"],
        vec!["nop", "cut", "1"],
        vec!["-g1", "cut", "2"],
        vec!["-g1", "reverse"],
        vec!["CuT", "1"],
        vec!["c\\ut", "1"],
        vec!["reverse", "count", "1"],
        vec!["--header-in", "cut", "y,x"],
        vec!["-H", "cut", "y,2,x"],
        vec!["-Hf", "echo", "y"],
        vec!["-H", "cut", "missing"],
        vec!["cut", "x"],
        vec!["--vnlog", "reverse"],
        vec!["--vnlog", "cut", "x,y"],
        vec!["--vnlog", "-f", "nop"],
        vec!["-z", "cut", "2,1"],
        vec!["-z", "reverse"],
        vec!["-zf", "noop"],
    ] {
        for input in [
            b"x\ty\na\tb\n".as_slice(),
            b"# x y\na b #tail\n",
            b"a\tb\0c\td\0",
            b"",
            b"x\ty\n",
        ] {
            cases.push((args(&a), input.to_vec()));
        }
    }
    for operation in ["cut", "echo"] {
        for flag in ["--narm", "-f", "-H", "--header-out"] {
            for input in [
                b"NA\tN/A\nNaN\t-\nx\ty\n".as_slice(),
                b"a\tb\nc\n",
                b"a\tb\nc\td\te\n",
            ] {
                cases.push((args(&[flag, operation, "2,1"]), input.to_vec()));
            }
        }
    }
    for mode in ["nop", "noop"] {
        for flags in [vec!["--vnlog", "-f"], vec!["--vnlog"], vec!["-Cf"]] {
            let mut a = args(&flags);
            a.push(mode.into());
            cases.push((
                a,
                b";keep\n\n  \n\0hidden\n # comment\nx #tail  \n".to_vec(),
            ));
        }
    }
    compare_cases(&cases, &reference, &evidence);
}
#[test]
fn field_modes_stream_wide_raw_records_and_report_output_failure() {
    for command in [vec!["cut", "2,1"], vec!["reverse"], vec!["-f", "noop"]] {
        let mut input = vec![0xff; 100_000];
        input.extend_from_slice(b"\tb\n");
        let output = invoke(&candidate(), &args(&command), &input, false, false);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let expected = if command[0] == "-f" {
            input.clone()
        } else {
            let mut expected = b"b\t".to_vec();
            expected.extend(vec![0xff; 100_000]);
            expected.push(b'\n');
            expected
        };
        assert_eq!(output.stdout, expected);
        let failed = invoke(&candidate(), &args(&command), &input, false, true);
        assert_eq!(failed.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&failed.stderr).contains("No space left on device"));
    }
    let input = b"a\tb\n".repeat(100_000);
    for command in [vec!["cut", "2,1"], vec!["reverse"], vec!["-f", "nop"]] {
        let output = invoke(&candidate(), &args(&command), &input, false, false);
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            if command[0] == "-f" {
                input.clone()
            } else {
                b"b\ta\n".repeat(100_000)
            }
        );
    }
}
