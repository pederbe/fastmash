//! Checks of how paired Operations share work inside the Operation set:
//! which followers reuse an earlier pair's samples, and reuse of an earlier
//! parse of the same field. The sharing is internal, so these are white-box.
use super::*;

#[test]
fn paired_followers_share_only_identical_ordered_selectors_and_reset() {
    let requests = grammar::parse(
        &["pcov", "1:2", "sum", "1", "dotprod", "1:2", "scov", "2:1"].map(Into::into),
    )
    .ok()
    .unwrap();
    let (mut set, _) = OperationSet::new(requests).ok().unwrap();
    set.bind([]).ok().unwrap();
    assert_eq!(set.plans[2].sample_source, Some(0));
    assert_eq!(set.plans[3].sample_source, None);
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
        set.collect_line(record, line as u64 + 1, &options, &mut arithmetic, None)
            .ok()
            .unwrap();
    }
    assert_eq!(set.states[0].kept().pair_samples.lengths(), (2, 2));
    assert_eq!(set.states[2].kept().pair_samples.capacities(), (0, 0));
    assert_eq!(
        set.result(2, &mut arithmetic, &options).ok().unwrap(),
        b"22"
    );
    set.reset();
    set.collect_line(b"3\t7", 1, &options, &mut arithmetic, None)
        .ok()
        .unwrap();
    assert_eq!(
        set.result(2, &mut arithmetic, &options).ok().unwrap(),
        b"21"
    );
    assert_eq!(set.states[0].kept().pair_samples.lengths(), (1, 1));
}

#[test]
fn pairs_reuse_an_earlier_parse_of_the_same_field() {
    let summarize_all = |words: &[&str], records: &[&[u8]]| {
        let requests = grammar::parse(&words.iter().map(|w| (*w).into()).collect::<Vec<_>>())
            .ok()
            .unwrap();
        let (mut set, _) = OperationSet::new(requests).ok().unwrap();
        set.bind([]).ok().unwrap();
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
            set.collect_line(record, line as u64 + 1, &options, &mut arithmetic, None)
                .ok()
                .unwrap();
        }
        let links: Vec<_> = set.plans.iter().map(|op| op.pair_conversion).collect();
        let results: Vec<_> = (0..set.len())
            .map(|at| set.result(at, &mut arithmetic, &options).ok().unwrap())
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
