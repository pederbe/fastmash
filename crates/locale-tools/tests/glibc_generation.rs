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
    // A newer normalization library is compatible when its actual decompositions
    // still match the retained data. Check those ranges below, not its version.
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

#[test]
fn emission_preserves_source_rank_scalar_order_and_cross_locale_intersection() {
    use fastmash_locale_tools::collation::Row;
    use fastmash_locale_tools::glibc_artifacts::emit;
    use fastmash_locale_tools::glibc_generation::Observation;
    use std::collections::BTreeMap;

    let common = Common {
        ignorable: vec![0x22, 0x20, 0x21],
        contractions: vec![("\u{FFFF}x".into(), 'x'), ("\u{10000}x".into(), 'y')],
    };
    let rows = vec![
        Row {
            name: "bb".into(),
            tag: "second".into(),
        },
        Row {
            name: "aa".into(),
            tag: "first".into(),
        },
    ];
    let mut observed = BTreeMap::from([
        (
            "bb".into(),
            Observation {
                weighed: vec![0x20],
                other_weighed: vec![0x22],
                no_fourth: vec![0x4E00, 0x4E01],
                reordered: BTreeMap::from([(0x0401, 0x0415)]),
            },
        ),
        (
            "aa".into(),
            Observation {
                weighed: Vec::new(),
                other_weighed: Vec::new(),
                no_fourth: vec![0x4E00, 0x4E02],
                reordered: BTreeMap::from([(0x0401, 0x0415)]),
            },
        ),
    ]);
    let decomposed = BTreeMap::from([(0x00C5, "A\u{30A}".into())]);
    let emitted = emit("own-common", b"abc", &common, &decomposed, &rows, &observed).unwrap();
    let text = std::str::from_utf8(&emitted.json).unwrap();
    assert!(text.find("FFFF 0078").unwrap() < text.find("10000 0078").unwrap());
    let json: serde_json::Value = serde_json::from_slice(&emitted.json).unwrap();
    assert_eq!(
        json["source_sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        json["no_fourth_everywhere"],
        serde_json::json!([[0x4E00, 0x4E00]])
    );
    assert_eq!(
        json["locales"]["bb"]["no_fourth"],
        serde_json::json!([[0x4E01, 0x4E01]])
    );
    assert_eq!(
        json["locales"]["bb"]["other_weighed"],
        serde_json::json!([0x22])
    );
    assert!(
        emitted
            .rust
            .contains("    (0x0020, 0x0021, 1),\n    (0x0022, 0x0022, 0),")
    );
    assert!(emitted.rust.contains("    (0x00C5, 0x00C5, 2),"));
    assert!(
        emitted
            .rust
            .contains(r#"    ("\u{FFFF}\u{0078}", '\u{0078}'),"#)
    );
    assert!(emitted.rust.find("(\"first\"").unwrap() < emitted.rust.find("(\"second\"").unwrap());
    assert!(emitted.json.ends_with(b"\n"));
    observed.get_mut("aa").unwrap().reordered.clear();
    assert!(emit("own-common", b"abc", &common, &decomposed, &rows, &observed).is_err());
}

#[test]
fn emission_refuses_incomplete_observations_inconsistent_tags_and_unrepresentable_tables() {
    use fastmash_locale_tools::collation::Row;
    use fastmash_locale_tools::glibc_artifacts::emit;
    use fastmash_locale_tools::glibc_generation::Observation;
    use std::collections::BTreeMap;

    let mut common = Common {
        ignorable: vec![32],
        contractions: Vec::new(),
    };
    let rows = vec![
        Row {
            name: "aa".into(),
            tag: "same".into(),
        },
        Row {
            name: "bb".into(),
            tag: "same".into(),
        },
    ];
    let empty = || Observation {
        weighed: Vec::new(),
        other_weighed: Vec::new(),
        no_fourth: Vec::new(),
        reordered: BTreeMap::new(),
    };
    let mut observed = BTreeMap::from([("aa".into(), empty())]);
    assert!(emit("common", b"", &common, &BTreeMap::new(), &rows, &observed).is_err());
    observed.insert("bb".into(), empty());
    observed.get_mut("bb").unwrap().no_fourth.push(0x00C5);
    assert!(emit("common", b"", &common, &BTreeMap::new(), &rows, &observed).is_err());
    observed.get_mut("bb").unwrap().no_fourth.clear();
    let long = BTreeMap::from([(0x00C5, "A".repeat(256))]);
    assert!(emit("common", b"", &common, &long, &rows, &observed).is_err());
    common.ignorable = (0..=0x10000).collect();
    assert!(emit("common", b"", &common, &BTreeMap::new(), &rows, &observed).is_err());
}
