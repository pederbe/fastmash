//! Ordinary completion checks through the Command interface.
use super::super::command_test_support::{Input, Output, command};

#[test]
fn typed_count_completion_preserves_all_unsigned_integer_bits() {
    // Independent expected encodings are retained in the immutable v3
    // primitive-finite packet's promote-u64 cases. Large counts are set at the
    // allowed typed completion seam rather than requiring impractical input.
    for (count, expected) in [
        (9_007_199_254_740_993, "40348000000000000400"),
        (u64::MAX, "403effffffffffffffff"),
    ] {
        let requests = super::super::grammar::parse(&["count".into(), "1".into()])
            .unwrap_or_else(|_| panic!("valid count"));
        let (mut operations, _) =
            super::OperationSet::new(requests).unwrap_or_else(|_| panic!("count setup"));
        operations
            .bind([])
            .unwrap_or_else(|_| panic!("numbered count"));
        operations.states[0].count = count;
        let super::super::options::Action::Calculate(options) =
            super::super::options::parse(&["count".into(), "1".into()], b"fastmash", false)
                .unwrap_or_else(|_| panic!("count options"))
        else {
            panic!("calculation")
        };
        let mut arithmetic = super::super::numerics::Numerics::new(false).unwrap();
        let value = operations
            .completion()
            .numerical_result(0, &mut arithmetic, &options)
            .unwrap_or_else(|_| panic!("completed count"));
        assert_eq!(
            super::super::numerics::Numerics::value80(value).raw(),
            fastmash_numeric_contract::Value80::from_hex(expected.as_bytes())
                .unwrap()
                .raw()
        );
    }
}

fn run(args: &[&str], bytes: &[u8]) -> (i32, Vec<u8>, Vec<Vec<u8>>) {
    let mut input = Input {
        bytes,
        segment: 3,
        error: None,
    };
    let mut output = Output {
        bytes: Vec::new(),
        error: None,
        closed: false,
    };
    let (status, diagnostics) = command(&mut input, &mut output, args);
    assert!(output.closed);
    (status, output.bytes, diagnostics)
}

#[test]
fn mixed_completion_preserves_shared_summaries_and_group_resets() {
    let operations = [
        "count",
        "2",
        "countunique",
        "2",
        "sum",
        "2",
        "mean",
        "2",
        "median",
        "2",
        "madraw",
        "2",
        "pvar",
        "2",
        "wmean",
        "2:3",
        "first",
        "1",
        "median",
        "2",
        "pvar",
        "2",
        "madraw",
        "2",
    ];
    for csv in [false, true] {
        let mut args = vec!["--narm", "-g", "1"];
        if csv {
            args.push("--csv");
        }
        args.extend_from_slice(&operations);
        let (input, expected): (&[u8], &[u8]) = if csv {
            (
                b"A,1,1\nA,3,1\nB,10,2\nB,NA,0\n",
                b"A,2,2,4,2,2,1,1,2,A,2,1,1\nB,1,1,10,10,10,0,0,10,B,10,0,0\n",
            )
        } else {
            (
                b"A\t1\t1\nA\t3\t1\nB\t10\t2\nB\tNA\t0\n",
                b"A\t2\t2\t4\t2\t2\t1\t1\t2\tA\t2\t1\t1\nB\t1\t1\t10\t10\t10\t0\t0\t10\tB\t10\t0\t0\n",
            )
        };
        assert_eq!(run(&args, input), (0, expected.to_vec(), vec![]));
    }
}

#[test]
fn empty_retained_results_keep_counts_extrema_and_special_values() {
    assert_eq!(
        run(
            &[
                "--narm",
                "count",
                "1",
                "countunique",
                "1",
                "sum",
                "1",
                "mean",
                "1",
                "min",
                "1",
                "max",
                "1",
                "range",
                "1",
                "median",
                "1",
                "madraw",
                "1",
                "pvar",
                "1",
                "first",
                "1",
            ],
            b"NA\nNaN\nN/A\n",
        ),
        (
            0,
            b"0\t0\t0\tnan\t-inf\tinf\tnan\tnan\tnan\tnan\tN/A\n".to_vec(),
            vec![]
        )
    );
}

#[test]
fn counts_and_text_results_keep_independent_format_controls() {
    for (format, expected) in [
        ("--format=%+.2f", b"+3.00\t+2.00\talpha\n".as_slice()),
        ("--round=3", b"3.000\t2.000\talpha\n"),
    ] {
        assert_eq!(
            run(
                &[format, "count", "1", "countunique", "1", "first", "1"],
                b"alpha\nbeta\nalpha\n"
            ),
            (0, expected.to_vec(), vec![])
        );
    }
}

#[test]
fn paired_failure_keeps_earlier_output_in_request_order() {
    let (status, output, diagnostics) = run(
        &["--narm", "count", "1", "dotprod", "1:2", "sum", "2"],
        b"1\tNA\n2\t3\n",
    );
    assert_eq!(status, 1);
    assert_eq!(output, b"2\t");
    assert_eq!(
        diagnostics,
        vec![
            b"input error for operation 'dotprod': fields 1,2 have different number of items\n"
                .to_vec()
        ]
    );
}
