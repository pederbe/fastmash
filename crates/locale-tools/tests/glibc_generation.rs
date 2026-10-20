#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::glibc_generation::{
    Common, decomposition_ranges, decompositions, letters,
};

#[allow(dead_code)]
#[path = "../../cli/src/collation_glibc.rs"]
mod retained;

#[test]
fn unicode_sixteen_normalization_matches_every_retained_decomposition_range() {
    assert_eq!(unicode_normalization::UNICODE_VERSION, (16, 0, 0));
    assert_eq!(unicode_general_category::UNICODE_VERSION, (16, 0, 0));
    let actual = decomposition_ranges(&decompositions());
    let expected: Vec<_> = retained::DECOMPOSED
        .iter()
        .map(|&(start, end, length)| (start, end, length as usize))
        .collect();
    assert_eq!(actual, expected);
    let candidates = letters();
    assert!(candidates.contains(&0x1C89));
    assert!(!candidates.contains(&0xD800));
    assert!(!candidates.contains(&0x20AC));
}

#[test]
fn own_format_fixtures_preserve_order_and_longest_contractions() {
    let text = "<U0021> IGNORE;IGNORE;IGNORE;<U0021>\n<U0020> IGNORE;IGNORE;IGNORE;<U0020>\ncollating-element <example> from \"<U0041><U030A>\" % decomposition of LATIN CAPITAL LETTER A WITH RING ABOVE\ncollating-element <longer> from \"<U0041><U030A><U0062>\" % decomposition of LATIN CAPITAL LETTER B\n";
    let common = Common::parse(text).unwrap();
    assert_eq!(common.ignorable, [0x21, 0x20]);
    assert_eq!(common.compose("xA\u{30a}bA\u{30a}é"), "xBÅé");
    assert!(Common::parse(&format!("{text}<U0020> IGNORE;IGNORE;IGNORE;<U0020>\n")).is_err());
    assert!(Common::parse(&format!("{text}{text}")).is_err());
    assert!(Common::parse("").is_err());
    assert!(
        Common::parse(
            "collating-element <bad> from \"<UD800>\" % decomposition of LATIN CAPITAL LETTER A\n"
        )
        .is_err()
    );
}

#[test]
fn probe_predicates_preserve_reordering_and_refuse_incomplete_native_batches() {
    use fastmash_locale_tools::glibc_generation::{Observation, probe};
    use std::cmp::Ordering::{Equal, Greater, Less};
    use std::collections::BTreeMap;

    let common = Common {
        ignorable: vec![0, 32, 33],
        contractions: vec![("x".into(), 'x')],
    };
    let decomposed = BTreeMap::from([(0x0401, "Е\u{308}".into())]);
    let actual = probe(&common, &[0x4E00, 0x00E9], &decomposed, |label, pairs| {
        Ok(match label {
            "contractions" => vec![Equal; pairs.len()],
            "decomposition-ties" | "decomposition-contexts" => vec![Greater; pairs.len()],
            "ignorable-levels" => vec![Less, Less, Less, Greater, Less, Greater],
            "weighed-below-ignorables" => vec![Less; pairs.len()],
            "weighed-order" => Vec::new(),
            "fourth-level-letters" => vec![Less, Equal],
            _ => panic!("unexpected phase"),
        })
    })
    .unwrap();
    assert_eq!(
        actual,
        Observation {
            weighed: vec![32],
            other_weighed: Vec::new(),
            no_fourth: vec![0x4E00],
            reordered: BTreeMap::from([(0x0401, 0x0415)])
        }
    );
    assert!(probe(&common, &[], &BTreeMap::new(), |_, _| Ok(Vec::new())).is_err());
    assert!(
        probe(&common, &[], &BTreeMap::new(), |_, pairs| Ok(vec![
            Less;
            pairs
                .len(
                )
        ]))
        .is_err()
    );
}
