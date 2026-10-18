#![forbid(unsafe_code)]

use fastmash_locale_tools::collation_plan::OrderDifference;
use fastmash_locale_tools::collation_verification::{CaseOutcome, Observation, Report};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const RETAINED: &[u8] = include_bytes!("../../../data/locales/collation-verification.json");
const CANDIDATES: &[u8] = include_bytes!("../../../data/locales/collation-candidates.json");
const BLOCKS: [&str; 29] = [
    "latin",
    "vietnamese",
    "greek",
    "cyrillic",
    "armenian",
    "hebrew",
    "arabic",
    "devanagari",
    "bengali",
    "gurmukhi",
    "gujarati",
    "oriya",
    "tamil",
    "telugu",
    "kannada",
    "malayalam",
    "sinhala",
    "thai",
    "lao",
    "tibetan",
    "myanmar",
    "georgian",
    "ethiopic",
    "khmer",
    "thaana",
    "hiragana",
    "katakana",
    "hangul",
    "han",
];

fn equal() -> CaseOutcome {
    Ok(OrderDifference {
        equal: true,
        displaced: Vec::new(),
    })
}

fn unequal(words: Vec<String>) -> CaseOutcome {
    Ok(OrderDifference {
        equal: false,
        displaced: words,
    })
}

fn words(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|word| (*word).to_owned()).collect()
}

fn scripts(entries: &[(&str, Option<&[&str]>)]) -> Vec<(String, Option<BTreeSet<String>>)> {
    entries
        .iter()
        .map(|(name, values)| ((*name).into(), values.map(words)))
        .collect()
}

fn observation(
    name: &str,
    alphabet: CaseOutcome,
    digits: CaseOutcome,
    scripts: Vec<(String, Option<BTreeSet<String>>)>,
) -> Observation {
    Observation {
        name: name.into(),
        alphabet,
        digits,
        scripts,
    }
}

fn matched(name: &str) -> Observation {
    observation(name, equal(), equal(), Vec::new())
}

fn candidates(locales: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"icu": "78", "glibc": "reference", "locales": locales})).unwrap()
}

fn value(report: &Report) -> Value {
    serde_json::from_slice(&report.json().unwrap()).unwrap()
}

// These helpers construct prior inputs, never expected output from new code.
fn stored(verified: bool, tag: &str, failures: &str, other: &str) -> String {
    let tag = serde_json::to_string(tag).unwrap();
    format!(
        r#"{{"verified":{verified},"tag":{tag},"failures":{failures},"other_scripts":{other}}}"#
    )
}

fn prior(variants: &str) -> String {
    format!(r#"{{"variants":{variants},"icu":"old","glibc":"old","locales":{{}}}}"#)
}

fn appears_in_order(text: &str, needles: &[&str]) {
    let mut remaining = text;
    for needle in needles {
        let (_, after) = remaining
            .split_once(needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"));
        remaining = after;
    }
}

#[test]
fn retained_en_us_subset_update_preserves_complete_document_and_negative_evidence() {
    let original: Value = serde_json::from_slice(RETAINED).unwrap();
    let baseline = &original["variants"]["0"]["baseline_displaced"];
    assert_eq!(baseline.as_object().unwrap().len(), BLOCKS.len());
    let scripts = BLOCKS
        .iter()
        .map(|&block| {
            let values = baseline[block]
                .as_array()
                .unwrap()
                .iter()
                .map(|word| word.as_str().unwrap().to_owned())
                .collect();
            (block.into(), Some(values))
        })
        .collect();
    let report = Report::update(
        Some(RETAINED),
        CANDIDATES,
        0,
        vec![observation("en_US", equal(), equal(), scripts)],
    )
    .unwrap();
    assert_eq!(report.json().unwrap(), RETAINED);
    assert_eq!(
        report.summary_line(),
        "1 of 1 locales verified with candidate 0; 194 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        "en_US: verified\n1 of 1 locales verified with candidate 0; 194 locales verified overall\n"
    );
    let actual = value(&report);
    assert_eq!(actual["locales"].as_object().unwrap().len(), 319);
    assert_eq!(actual["variants"].as_object().unwrap().len(), 2);
    assert_eq!(
        actual["variants"]["0"]["locales"]
            .as_object()
            .unwrap()
            .len(),
        319
    );
    assert_eq!(
        actual["variants"]["1"]["locales"]
            .as_object()
            .unwrap()
            .len(),
        319
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["am_ET"]["failures"]["alphabet"]["displaced"],
        437
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["am_ET"]["failures"]["alphabet"]["first"]
            .as_array()
            .unwrap()
            .len(),
        40
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["ps_AF"]["failures"],
        original["variants"]["0"]["locales"]["ps_AF"]["failures"]
    );
    assert_eq!(
        actual["variants"]["1"]["locales"]["ps_AF"]["failures"],
        original["variants"]["1"]["locales"]["ps_AF"]["failures"]
    );
    assert_eq!(actual["locales"]["am_ET"]["tag"], "und");
    assert!(actual["locales"]["cs_CZ"]["tag"].is_null());
}

#[test]
fn literal_baseline_differences_promote_only_null_alphabets_and_keep_failed_blocks() {
    let candidates = candidates(json!({
        "en_US": {"tags":["base"],"alphabet":[]},
        "null": {"tags":["null-tag"],"alphabet":null},
        "empty": {"tags":["empty-tag"],"alphabet":[]},
        "subset": {"tags":["subset-tag"],"alphabet":["a"]}
    }));
    let baseline = scripts(&[
        ("z-script", Some(&["base", "shared"])),
        ("a-script", Some(&[])),
    ]);
    let changed = scripts(&[("z-script", Some(&["new", "shared"])), ("a-script", None)]);
    let report = Report::update(
        None,
        &candidates,
        0,
        vec![
            observation(
                "null",
                unequal(Vec::new()),
                Err(["candidate\né".into(), "reference \"bad\"".into()]),
                changed.clone(),
            ),
            observation("empty", equal(), equal(), changed),
            observation(
                "subset",
                equal(),
                equal(),
                scripts(&[("z-script", Some(&["shared"])), ("a-script", Some(&[]))]),
            ),
            observation("en_US", equal(), equal(), baseline),
        ],
    )
    .unwrap();
    let actual = value(&report);
    let null = &actual["variants"]["0"]["locales"]["null"];
    assert_eq!(null["verified"], false);
    assert_eq!(
        null["failures"],
        json!({
            "alphabet":{"displaced":0,"first":[]},
            "digits":{"error":["candidate\né","reference \"bad\""]},
            "block:z-script":1,"block:a-script":"error"
        })
    );
    assert_eq!(
        null["other_scripts"],
        json!({"z-script":1,"a-script":"error"})
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["empty"]["verified"],
        true
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["empty"]["other_scripts"],
        null["other_scripts"]
    );
    assert_eq!(
        actual["variants"]["0"]["locales"]["subset"]["other_scripts"],
        json!({})
    );
    assert_eq!(actual["locales"]["null"]["alphabet_data"], false);
    assert_eq!(actual["locales"]["empty"]["alphabet_data"], true);
    let text = String::from_utf8(report.json().unwrap()).unwrap();
    appears_in_order(
        text.split_once("\"null\": {").unwrap().1,
        &[
            "\"alphabet\":",
            "\"digits\":",
            "\"block:z-script\":",
            "\"block:a-script\":",
        ],
    );
    assert_eq!(
        report.summary_line(),
        "3 of 4 locales verified with candidate 0; 3 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        concat!(
            "empty: verified\nen_US: verified\n",
            "null: differs alphabet:0 digits:{'error': ['candidate\\né', 'reference \"bad\"']} ",
            "block:z-script:1 block:a-script:error\n",
            "subset: verified\n",
            "3 of 4 locales verified with candidate 0; 3 locales verified overall\n",
        )
    );
}

#[test]
fn new_case_records_keep_full_counts_first_forty_and_unequal_empty_failures() {
    let candidates = candidates(json!({
        "en_US":{"tags":["base"],"alphabet":[]},
        "many":{"tags":["many-tag"],"alphabet":[]}
    }));
    let moved = (0..47).map(|index| format!("word-{index:02}")).collect();
    let report = Report::update(
        None,
        &candidates,
        0,
        vec![
            matched("en_US"),
            observation("many", unequal(moved), unequal(Vec::new()), Vec::new()),
        ],
    )
    .unwrap();
    let actual = value(&report);
    let failures = &actual["variants"]["0"]["locales"]["many"]["failures"];
    assert_eq!(failures["alphabet"]["displaced"], 47);
    assert_eq!(
        failures["alphabet"]["first"],
        json!([
            "word-00", "word-01", "word-02", "word-03", "word-04", "word-05", "word-06", "word-07",
            "word-08", "word-09", "word-10", "word-11", "word-12", "word-13", "word-14", "word-15",
            "word-16", "word-17", "word-18", "word-19", "word-20", "word-21", "word-22", "word-23",
            "word-24", "word-25", "word-26", "word-27", "word-28", "word-29", "word-30", "word-31",
            "word-32", "word-33", "word-34", "word-35", "word-36", "word-37", "word-38", "word-39"
        ])
    );
    assert_eq!(failures["digits"], json!({"displaced":0,"first":[]}));
    assert_eq!(
        report.summary_line(),
        "1 of 2 locales verified with candidate 0; 1 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        concat!(
            "en_US: verified\nmany: differs alphabet:47 digits:0\n",
            "1 of 2 locales verified with candidate 0; 1 locales verified overall\n",
        )
    );
}

#[test]
fn partial_updates_preserve_dynamic_positions_untouched_failures_and_orphan_history() {
    let z = stored(
        false,
        "old-z",
        r#"{"alphabet":{"displaced":100,"first":["prefix"]}}"#,
        "{}",
    );
    let en = stored(true, "old-en", "{}", "{}");
    let untouched = stored(
        false,
        "untouched",
        r#"{"digits":{"displaced":90,"first":["z","a","z"]},"alphabet":{"error":["kept",""]}}"#,
        "{}",
    );
    let orphan = stored(true, "orphan-tag", "{}", "{}");
    let one = format!(
        r#"{{"locales":{{"orphan":{orphan}}},"baseline_displaced":{{"z-block":["z","a","z"],"a-block":[]}}}}"#
    );
    let zero = format!(r#"{{"locales":{{"z":{z},"en_US":{en},"untouched":{untouched}}}}}"#);
    let variants = format!(r#"{{"1":{one},"0":{zero},"-1":{{"locales":{{}}}}}}"#);
    let prior = prior(&variants);
    let candidates = candidates(json!({
        "a":{"tags":["new-a"],"alphabet":null}, "b":{"tags":["new-b"],"alphabet":[]},
        "en_US":{"tags":["new-en"],"alphabet":[]}, "z":{"tags":["new-z"],"alphabet":[]},
        "untouched":{"tags":["untouched"],"alphabet":[]}
    }));
    let report = Report::update(
        Some(prior.as_bytes()),
        &candidates,
        0,
        vec![matched("b"), matched("en_US"), matched("a"), matched("z")],
    )
    .unwrap();
    let actual = value(&report);
    assert_eq!(actual["variants"]["0"]["locales"]["z"]["tag"], "new-z");
    assert_eq!(
        actual["variants"]["0"]["locales"]["untouched"]["failures"],
        json!({
            "digits":{"displaced":90,"first":["z","a","z"]},"alphabet":{"error":["kept",""]}
        })
    );
    assert_eq!(
        actual["variants"]["1"]["locales"]["orphan"]["tag"],
        "orphan-tag"
    );
    assert!(actual["locales"].get("orphan").is_none());
    assert_eq!(actual["variants"]["-1"]["locales"], json!({}));
    assert_eq!(
        actual["variants"]["1"]["baseline_displaced"],
        json!({
            "z-block":["z","a","z"],"a-block":[]
        })
    );
    assert!(actual["variants"]["-1"].get("baseline_displaced").is_none());
    let text = String::from_utf8(report.json().unwrap()).unwrap();
    appears_in_order(&text, &["\"1\":", "\"0\":", "\"-1\":"]);
    let saved_baseline = text.split_once("\"baseline_displaced\":").unwrap().1;
    appears_in_order(saved_baseline, &["\"z-block\":", "\"a-block\":"]);
    let selected = text.split_once("\"0\":").unwrap().1;
    appears_in_order(
        selected,
        &[
            "\"z\":",
            "\"en_US\":",
            "\"untouched\":",
            "\"a\":",
            "\"b\":",
            "\"baseline_displaced\":",
        ],
    );
    let untouched = selected.split_once("\"untouched\":").unwrap().1;
    appears_in_order(untouched, &["\"digits\":", "\"alphabet\":"]);
    assert_eq!(
        report.summary_line(),
        "4 of 4 locales verified with candidate 0; 4 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        concat!(
            "a: verified\nb: verified\nen_US: verified\nz: verified\n",
            "4 of 4 locales verified with candidate 0; 4 locales verified overall\n",
        )
    );
}

#[test]
fn first_verified_selection_uses_positive_preference_stored_and_empty_tags() {
    let zero_en = stored(true, "saved-en", "{}", "{}");
    let zero_a = stored(true, "stale-a", "{}", "{}");
    let zero_b = stored(false, "failed-b", "{}", "{}");
    let zero_c = stored(true, "", "{}", "{}");
    let one_a = stored(true, "later-a", "{}", "{}");
    let one_b = stored(true, "second-b", "{}", "{}");
    let one_c = stored(true, "later-c", "{}", "{}");
    let variants = format!(
        r#"{{"1":{{"locales":{{"a":{one_a},"b":{one_b},"c":{one_c}}}}},"0":{{"locales":{{"en_US":{zero_en},"a":{zero_a},"b":{zero_b},"c":{zero_c}}}}}}}"#
    );
    let prior = prior(&variants);
    let candidates = candidates(json!({
        "en_US":{"tags":["current-en0","current-en1"],"alphabet":[]},
        "a":{"tags":["current-a0","current-a1"],"alphabet":[]},
        "b":{"tags":["current-b0","current-b1"],"alphabet":[]},
        "c":{"tags":["current-c0","current-c1"],"alphabet":[]},
        "d":{"tags":[],"alphabet":null}
    }));
    let report = Report::update(
        Some(prior.as_bytes()),
        &candidates,
        -1,
        vec![matched("en_US")],
    )
    .unwrap();
    let actual = value(&report);
    assert_eq!(actual["locales"]["en_US"]["tag"], "saved-en");
    assert_eq!(actual["locales"]["a"]["tag"], "stale-a");
    assert_eq!(actual["locales"]["b"]["tag"], "second-b");
    assert_eq!(actual["locales"]["c"]["tag"], "");
    assert!(actual["locales"]["d"]["tag"].is_null());
    assert_eq!(
        actual["variants"]["-1"]["locales"]["en_US"]["tag"],
        "current-en1"
    );
    assert_eq!(
        report.summary_line(),
        "1 of 1 locales verified with candidate -1; 3 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        "en_US: verified\n1 of 1 locales verified with candidate -1; 3 locales verified overall\n"
    );
}

#[test]
fn indexing_only_observed_names_and_failure_replacement_keep_run_counters() {
    let candidates = candidates(json!({
        "en_US":{"tags":["en0","en1"],"alphabet":[]},
        "short":{"tags":["short0"],"alphabet":[]}
    }));
    let report = Report::update(None, &candidates, 1, vec![matched("en_US")]).unwrap();
    assert!(value(&report)["locales"]["short"]["tag"].is_null());
    assert_eq!(
        report.summary_line(),
        "1 of 1 locales verified with candidate 1; 1 locales verified overall"
    );
    for variant in [1, -2, i64::MIN, i64::MAX] {
        assert!(
            Report::update(
                None,
                &candidates,
                variant,
                vec![matched("en_US"), matched("short")]
            )
            .is_err()
        );
    }
    let zero = stored(true, "zero", "{}", "{}");
    let one = stored(true, "one", "{}", "{}");
    let prior = prior(&format!(
        r#"{{"0":{{"locales":{{"en_US":{zero}}}}},"1":{{"locales":{{"en_US":{one}}}}}}}"#
    ));
    let report = Report::update(
        Some(prior.as_bytes()),
        &candidates,
        0,
        vec![observation(
            "en_US",
            unequal(Vec::new()),
            equal(),
            Vec::new(),
        )],
    )
    .unwrap();
    assert_eq!(value(&report)["locales"]["en_US"]["tag"], "one");
    assert_eq!(
        report.summary_line(),
        "0 of 1 locales verified with candidate 0; 1 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        "en_US: differs alphabet:0\n0 of 1 locales verified with candidate 0; 1 locales verified overall\n"
    );
}

#[test]
fn duplicate_equivalence_is_checked_after_aggregation_without_discarded_traces() {
    let candidates = candidates(json!({
        "en_US":{"tags":["en"],"alphabet":[]}, "dup":{"tags":["dup"],"alphabet":[]}
    }));
    let duplicate = |tail: &str, reversed: bool| {
        let mut moved: Vec<_> = (0..40).map(|index| format!("w{index:02}")).collect();
        moved.extend((40..47).map(|index| format!("{tail}{index:02}")));
        let mut blocks = scripts(&[("z", Some(&[])), ("a", Some(&[]))]);
        if reversed {
            blocks.reverse();
        }
        observation(
            "dup",
            unequal(moved),
            Ok(OrderDifference {
                equal: true,
                displaced: vec![format!("discarded-{tail}")],
            }),
            blocks,
        )
    };
    let baseline = || {
        observation(
            "en_US",
            equal(),
            equal(),
            scripts(&[("z", Some(&[])), ("a", Some(&[]))]),
        )
    };
    let report = Report::update(
        None,
        &candidates,
        0,
        vec![duplicate("x", false), baseline(), duplicate("y", true)],
    )
    .unwrap();
    assert_eq!(
        report.summary_line(),
        "1 of 2 locales verified with candidate 0; 1 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        concat!(
            "dup: differs alphabet:47\ndup: differs alphabet:47\nen_US: verified\n",
            "1 of 2 locales verified with candidate 0; 1 locales verified overall\n",
        )
    );
    assert_eq!(
        value(&report)["variants"]["0"]["locales"]["dup"]["failures"]["alphabet"]["displaced"],
        47
    );
    let conflict = observation(
        "dup",
        unequal(vec!["different".into()]),
        equal(),
        scripts(&[("z", Some(&[])), ("a", Some(&[]))]),
    );
    let error = Report::update(
        None,
        &candidates,
        0,
        vec![baseline(), duplicate("x", false), conflict],
    )
    .unwrap_err();
    assert!(error.to_string().contains("conflicting aggregated"));
    let report = Report::update(None, &candidates, 0, vec![baseline(), baseline()]).unwrap();
    assert_eq!(
        report.summary_line(),
        "1 of 1 locales verified with candidate 0; 1 locales verified overall"
    );
    assert_eq!(
        report.console_output(),
        concat!(
            "en_US: verified\nen_US: verified\n",
            "1 of 1 locales verified with candidate 0; 1 locales verified overall\n",
        )
    );
}

#[test]
fn complete_console_and_json_match_the_independent_error_observation() {
    let raw = [
        include_bytes!("fixtures/verification-console/candidate-alphabet.stderr").as_slice(),
        include_bytes!("fixtures/verification-console/reference-alphabet.stderr").as_slice(),
        include_bytes!("fixtures/verification-console/candidate-digits.stderr").as_slice(),
        include_bytes!("fixtures/verification-console/reference-digits.stderr").as_slice(),
    ];
    // Decode original raw stderr as the executor will. Expected output is the
    // complete unchanged producer capture, including its replacement behavior.
    let [
        candidate_alphabet,
        reference_alphabet,
        candidate_digits,
        reference_digits,
    ] = raw.map(|bytes| String::from_utf8_lossy(bytes).into_owned());
    let baseline = BLOCKS
        .iter()
        .map(|&block| (block.into(), Some(BTreeSet::new())))
        .collect();
    let report = Report::update(
        None,
        include_bytes!("fixtures/verification-console/candidates.json"),
        0,
        vec![observation(
            "en_US",
            Err([candidate_alphabet, reference_alphabet]),
            Err([candidate_digits, reference_digits]),
            baseline,
        )],
    )
    .unwrap();
    assert_eq!(
        report.console_output().as_bytes(),
        include_bytes!("fixtures/verification-console/errors.stdout")
    );
    assert_eq!(
        report.json().unwrap(),
        include_bytes!("fixtures/verification-console/errors.json")
    );
}

#[test]
fn absent_failed_or_incomplete_required_baselines_never_become_empty() {
    let missing_alphabet = candidates(json!({"en_US":{"tags":["en"]}}));
    let candidates = candidates(json!({
        "en_US":{"tags":["en"],"alphabet":[]}, "other":{"tags":["other"],"alphabet":[]}
    }));
    assert!(Report::update(None, &candidates, 0, vec![matched("other")]).is_err());
    assert!(
        Report::update(
            None,
            &candidates,
            0,
            vec![
                observation("en_US", equal(), equal(), scripts(&[("x", None)])),
                matched("en_US"),
            ]
        )
        .is_err()
    );
    assert!(
        Report::update(
            None,
            &candidates,
            0,
            vec![
                matched("en_US"),
                observation("other", equal(), equal(), scripts(&[("x", Some(&[]))])),
            ]
        )
        .is_err()
    );
    assert!(Report::update(None, &missing_alphabet, 0, vec![matched("en_US")]).is_err());
    assert!(
        Report::update(
            None,
            &candidates,
            0,
            vec![matched("en_US"), matched("unknown")]
        )
        .is_err()
    );
}

#[test]
fn optional_metadata_appends_in_producer_order_without_moving_existing_fields() {
    let candidates = candidates(json!({"en_US":{"tags":["en"],"alphabet":[]}}));
    for (prior, keys) in [
        (
            r#"{"variants":{}}"#,
            vec!["variants", "icu", "glibc", "locales"],
        ),
        (
            r#"{"variants":{},"locales":{},"glibc":"old"}"#,
            vec!["variants", "locales", "glibc", "icu"],
        ),
        (
            r#"{"variants":{},"icu":"old","locales":{}}"#,
            vec!["variants", "icu", "locales", "glibc"],
        ),
    ] {
        let report = Report::update(
            Some(prior.as_bytes()),
            &candidates,
            0,
            vec![matched("en_US")],
        )
        .unwrap();
        let text = String::from_utf8(report.json().unwrap()).unwrap();
        let needles: Vec<_> = keys.iter().map(|key| format!("\n \"{key}\":")).collect();
        let borrowed: Vec<_> = needles.iter().map(String::as_str).collect();
        appears_in_order(&text, &borrowed);
        assert_eq!(value(&report)["icu"], "78");
        assert_eq!(value(&report)["glibc"], "reference");
    }
}

#[test]
fn unsupported_prior_orders_shapes_fields_and_duplicate_json_keys_are_refused() {
    let candidates = candidates(json!({"en_US":{"tags":["en"],"alphabet":[]}}));
    let invalid = [
        r#"{}"#,
        r#"{"icu":"old","variants":{}}"#,
        r#"{"variants":{},"unknown":1}"#,
        r#"{"variants":{},"variants":{}}"#,
        r#"{"variants":{"0":{"baseline_displaced":{},"locales":{}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"tag":"en","verified":true,"failures":{},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":true,"tag":"en","failures":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":true,"tag":"en","failures":{},"other_scripts":{},"unknown":1}}}}}"#,
        r#"{"variants":{},"locales":{"en_US":{"alphabet_data":true,"tag":"en"}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":false,"tag":"en","failures":{"alphabet":{"first":[],"displaced":0}},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":false,"tag":"en","failures":{"digits":{"error":["one"]}},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":false,"tag":"en","failures":{"unknown":1},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{"en_US":{"verified":false,"tag":"en","failures":{"alphabet":1},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"0":{"locales":{},"extra":{}}}}"#,
        r#"{"variants":{"1":{"locales":{"saved":{"verified":false,"tag":"saved","failures":{"alphabet":[["candidate","reference"]]},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"1":{"locales":{"saved":{"verified":false,"tag":"saved","failures":{"block:latin":{"error":null}},"other_scripts":{}}}}}}"#,
        r#"{"variants":{"1":{"locales":{"saved":{"verified":false,"tag":"saved","failures":{},"other_scripts":{"latin":{"error":null}}}}}}}"#,
    ];
    for prior in invalid {
        assert!(
            Report::update(
                Some(prior.as_bytes()),
                &candidates,
                0,
                vec![matched("en_US")]
            )
            .is_err(),
            "{prior}"
        );
    }
}
