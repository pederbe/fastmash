use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn grouped_lookup_preserves_values_and_failure_order() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for prefix in [vec![], vec!["-g", "1"], vec!["-s", "-g", "1"]] {
        for operations in [
            vec![
                "count", "2", "min", "2", "max", "2", "sum", "2", "mean", "2", "median", "2",
            ],
            vec!["sum", "2", "count", "4"],
            vec!["count", "4", "sum", "2"],
            vec!["min", "3", "max", "2", "mean", "3"],
            vec!["first", "3", "min", "2", "last", "3"],
            vec!["ms", "2", "min", "2", "sum", "2", "mean", "2", "max", "2"],
            vec![
                "count", "3", "dotprod", "2:3", "dotprod", "3:2", "dotprod", "2:2", "sum", "2",
            ],
            vec!["dotprod", "2:4", "sum", "3"],
            vec!["dotprod", "4:2", "sum", "3"],
        ] {
            for input in [
                b"a\t1.25\t5\na\t3.75\t7\nb\t2\t9\n".as_slice(),
                b"a\tbad\n",
                b"a\t\t\n",
                b"a\t1\t2\t\n",
                b"a\tNA\t3\na\tN/A\t5\na\t1\t7\n",
                b"a\t2\t3\na\tNA\t5\na\t4\t7\nb\t1\t2\n",
                b"a\t1\0tail\t2\n",
                b"a\t1e\t2\n",
                b"\n",
            ] {
                for narm in [false, true] {
                    let mut a = prefix.clone();
                    if narm {
                        a.push("--narm");
                    }
                    a.extend(operations.iter().copied());
                    cases.push((args(&a), input.to_vec()));
                }
            }
        }
    }
    for (a, input) in [
        (
            vec!["-H", "-g", "key", "sum", "value", "count", "tail"],
            b"key\tvalue\ttail\na\t2\tx\na\t3\ty\n".as_slice(),
        ),
        (vec!["--header-out", "sum", "2", "count", "5"], b"a\tb\tc\n"),
        (vec!["-g", "4,1", "sum", "2"], b"a\t1\t2\tx\na\tbad\n"),
        (
            vec!["-f", "-g", "1", "min", "2"],
            b"a\t3\tfirst\na\t1\tchosen\na\t1\ttie\n",
        ),
        (
            vec!["-W", "-g", "1", "count", "3", "sum", "2"],
            b" a  1 \t\na\t2 \n",
        ),
        (vec!["-t", "e", "sum", "1", "sum", "2"], b"1e2\n3e4\n"),
        (vec!["-t", ".", "sum", "1", "sum", "2"], b"1.25\n3.75\n"),
        (
            vec!["-t", "e", "min", "1", "mean", "1", "max", "1"],
            b"1e2\n3e4\n",
        ),
        (
            vec!["-t", ".", "min", "1", "mean", "1", "max", "1"],
            b"1.25\n3.75\n",
        ),
        (
            vec![
                "-H", "--narm", "-g", "key", "ms", "value", "min", "2", "sum", "value", "mean",
                "2", "median", "value", "max", "2",
            ],
            b"key\tvalue\na\t2\na\tNA\na\t5\nb\tNA\nb\t3\n",
        ),
        (
            vec!["-z", "-g", "1", "sum", "2", "first", "3"],
            b"a\t1\tline\nbreak\0a\t2\tlast\0",
        ),
    ] {
        cases.push((args(&a), input.to_vec()));
    }
    let wide = [
        b"a\t2\t".as_slice(),
        &vec![b'x'; 100_000],
        b"\na\t4\ttail\n",
    ]
    .concat();
    cases.push((
        args(&["-g", "1", "sum", "2", "mean", "2", "count", "3"]),
        wide,
    ));
    compare_cases(&cases, &reference, &evidence);
}
