use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn fractional_decimal_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for coefficient in [
        0u64,
        1,
        3,
        (1 << 53) - 1,
        (1 << 53) + 1,
        (1 << 63) - 1,
        9_999_999_999_999_999_999,
    ] {
        for scale in 1..=19 {
            for sign in ["", "-"] {
                let value = format!("{sign}{coefficient}e-{scale}");
                cases.push((
                    args(&["sum", "1", "mean", "1", "min", "1", "max", "1"]),
                    format!("{value}\n").into_bytes(),
                ));
            }
        }
    }
    for input in [
        b"0.1\n0.2\n-0.3\n".as_slice(),
        b"9007199254740993e-1\n-9007199254740992e-1\n",
        b"9999999999999999999e-19\n1e-19\n-1\n",
        b"1.2345678901234567890123456789\n0x1p-64\n1e-20\n",
        b"-0\n-0.000e-19\n",
        b"1e4933\n",
        b"1e-5000\n",
        b"1e-1junk\n",
        b"1e-\n",
        b"NA\n0.125\nN/A\n",
    ] {
        for narm in [false, true] {
            let mut a = args(&["sum", "1", "mean", "1"]);
            if narm {
                a.insert(0, "--narm".into());
            }
            cases.push((a, input.to_vec()));
        }
    }
    for sorted in [false, true] {
        let mut a = args(&[
            "-H", "-g", "key", "sum", "value", "mean", "value", "min", "value", "max", "value",
            "median", "value",
        ]);
        if sorted {
            a.insert(0, "-s".into());
        }
        cases.push((
            a,
            b"key\tvalue\ttail\na\t0.1\tx\na\t0.2\ty\na\t-0.3\tz\nb\t9999999999999999999e-19\tt\n"
                .to_vec(),
        ));
    }
    // Numeric conversion initially sees the suffix, then retries the selected field.
    cases.push((
        args(&["-t", ".", "sum", "1", "sum", "2"]),
        b"1.25\n3.75\n".to_vec(),
    ));
    compare_cases(&cases, &reference, &evidence);
}
