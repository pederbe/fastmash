use super::*;

#[test]
fn paired_followers_share_only_identical_ordered_selectors_and_reset() {
    let requests = grammar::parse(
        &["pcov", "1:2", "sum", "1", "dotprod", "1:2", "scov", "2:1"].map(Into::into),
    )
    .ok()
    .unwrap();
    let (mut ops, _) = operations(requests).ok().unwrap();
    link_shared_fields(&mut ops);
    assert_eq!(ops[2].sample_source, Some(0));
    assert_eq!(ops[3].sample_source, None);
    let options::Action::Calculate(options) = options::parse(
        &["--narm".into(), "pcov".into(), "1:2".into()],
        b"fastmash",
        false,
    )
    .ok()
    .unwrap() else {
        panic!()
    };
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    for (line, record) in [b"NA\t1".as_slice(), b"2\tNA", b"4\t5"].iter().enumerate() {
        collect(
            record,
            &mut ops,
            line as u64 + 1,
            &options,
            &mut arithmetic,
            None,
        )
        .ok()
        .unwrap();
    }
    assert_eq!(ops[0].pair_samples.as_ref().unwrap().lengths(), (2, 2));
    assert_eq!(ops[2].pair_samples.as_ref().unwrap().capacities(), (0, 0));
    assert_eq!(
        summarize_at(
            &mut ops,
            2,
            &mut arithmetic,
            b',',
            &options.presentation,
            false
        )
        .ok()
        .unwrap(),
        b"22"
    );
    for op in &mut ops {
        op.reset();
    }
    collect(b"3\t7", &mut ops, 1, &options, &mut arithmetic, None)
        .ok()
        .unwrap();
    assert_eq!(
        summarize_at(
            &mut ops,
            2,
            &mut arithmetic,
            b',',
            &options.presentation,
            false
        )
        .ok()
        .unwrap(),
        b"21"
    );
    assert_eq!(ops[0].pair_samples.as_ref().unwrap().lengths(), (1, 1));
}

#[test]
fn pairs_reuse_an_earlier_parse_of_the_same_field() {
    let summarize_all = |words: &[&str], records: &[&[u8]]| {
        let requests = grammar::parse(&words.iter().map(|w| (*w).into()).collect::<Vec<_>>())
            .ok()
            .unwrap();
        let (mut ops, _) = operations(requests).ok().unwrap();
        link_shared_fields(&mut ops);
        let options::Action::Calculate(options) = options::parse(
            &["--narm".into(), "count".into(), "1".into()],
            b"fastmash",
            false,
        )
        .ok()
        .unwrap() else {
            panic!()
        };
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        for (line, record) in records.iter().enumerate() {
            collect(
                record,
                &mut ops,
                line as u64 + 1,
                &options,
                &mut arithmetic,
                None,
            )
            .ok()
            .unwrap();
        }
        let links: Vec<_> = ops.iter().map(|op| op.pair_conversion).collect();
        let results: Vec<_> = (0..ops.len())
            .map(|at| {
                summarize_at(
                    &mut ops,
                    at,
                    &mut arithmetic,
                    b',',
                    &options.presentation,
                    false,
                )
                .ok()
                .unwrap()
            })
            .collect();
        (links, results)
    };
    let records: &[&[u8]] = &[b"1\t2", b"NA\t5", b"4\tNA", b"7\t9"];
    let (links, shared) = summarize_all(&["mean", "1", "sum", "2", "pcov", "1:2"], records);
    assert_eq!(links[2], [Some(0), Some(1)]);
    let (links, alone) = summarize_all(&["pcov", "1:2"], records);
    assert_eq!(links[0], [None, None]);
    assert_eq!(shared[2], alone[0]);
    // A later single request does not feed an earlier pair.
    let (links, _) = summarize_all(&["pcov", "1:2", "mean", "1"], records);
    assert_eq!(links[0], [None, None]);
}
