use super::*;

const ALGORITHMS: [&str; 6] = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"];
// The abc vectors from RFC1321 and RFC6234, independent of the candidate library.
pub(super) const ABC: [&str; 6] = [
    "900150983cd24fb0d6963f7d28e17f72",
    "a9993e364706816aba3e25717850c26c9cd0d89d",
    "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
];

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn checksum_jobs_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for op in ALGORITHMS {
        for value in [
            b"".as_slice(),
            b"abc",
            b"a\0b",
            b"\xff\xfe",
            b"NA",
            b"NaN",
            b"N/A",
            b" a ",
        ] {
            for flags in [
                vec!["-t|"],
                vec!["-t|", "--narm"],
                vec!["-t|", "--header-out"],
                vec!["-t|", "-f", "--output-delimiter=;"],
            ] {
                let mut a = args(&flags);
                a.extend(args(&[op, "1,2"]));
                let mut input = value.to_vec();
                input.extend_from_slice(b"|abc\n");
                cases.push((a, input));
            }
        }
        for length in [
            1, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 255, 256, 257, 4096, 1_000_000,
        ] {
            let mut input = vec![b'a'; length];
            input.extend_from_slice(b"\nabc\n");
            cases.push((args(&[op, "1"]), input));
        }
        for a in [
            vec!["-Hf", op, "value,other"],
            vec!["--vnlog", op, "value"],
            vec!["-s", op, "1"],
            vec!["-g1", op, "2"],
            vec![op, "1:2"],
            vec![op, "1", "count", "1"],
            vec![op, "1", "debase64", "2"],
        ] {
            cases.push((args(&a), b"value\tother\nabc\t!\n".to_vec()));
        }
        for parameter in ["1", "1.5", "x", "", "1:2"] {
            cases.push((
                args(&[&format!("{op}:{parameter}"), "1"]),
                b"abc\n".to_vec(),
            ));
        }
        cases.push((args(&["-z", op, "1"]), b"a\nb\0\xff\0".to_vec()));
        cases.push((args(&[op, "1", op, "3"]), b"a\tb\tc\nx\ty\n".to_vec()));
        for full in [false, true] {
            let expected = super::transpose::failing_input(&reference, &[op, "1"], full);
            let actual = super::transpose::failing_input(&candidate(), &[op, "1"], full);
            fs::write(
                evidence.join(format!("{op}-pty-{full}.gnu.txt")),
                format!("{expected:?}"),
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{op}-pty-{full}.candidate.txt")),
                format!("{actual:?}"),
            )
            .unwrap();
            assert_eq!(actual.stdout, expected.stdout);
            assert_eq!(actual.stderr, expected.stderr);
            assert_eq!(actual.status, expected.status);
        }
    }
    let mut all_bytes: Vec<u8> = (0..=255).filter(|&b| b != b'\n' && b != b'|').collect();
    all_bytes.push(b'\n');
    for op in ALGORITHMS {
        cases.push((args(&["-t|", op, "1"]), all_bytes.clone()));
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn checksum_jobs_vectors_and_output_failures() {
    for (op, digest) in ALGORITHMS.into_iter().zip(ABC) {
        let out = invoke(&candidate(), &args(&[op, "1"]), b"abc\n", false, false);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, format!("{digest}\n").as_bytes());
        assert!(out.stderr.is_empty());
        let out = invoke(&candidate(), &args(&[op, "1"]), b"abc\n", false, true);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("write error"));
    }
    let mut request = Vec::new();
    for op in ALGORITHMS {
        request.extend(args(&[op, "1"]));
    }
    let out = invoke(
        &candidate(),
        &request,
        &b"abc\n".repeat(100_000),
        false,
        false,
    );
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        format!("{}\n", ABC.join("\t")).repeat(100_000).as_bytes()
    );
}
