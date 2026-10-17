#![forbid(unsafe_code)]

use fastmash_locale_tools::collation_plan::OrderDifference;
use fastmash_locale_tools::collation_punctuation::{PunctuationOutcome, Report};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const RETAINED: &[u8] = include_bytes!("../../../data/locales/collation-punctuation.json");

fn compared(words: &[&str], equal: bool) -> PunctuationOutcome {
    PunctuationOutcome::Compared(OrderDifference {
        equal,
        displaced: words.iter().map(|word| (*word).to_owned()).collect(),
    })
}

#[test]
fn all_194_retained_negative_observations_round_trip_with_exact_original_bytes() {
    #[derive(Deserialize)]
    struct Historical {
        words: usize,
        locales: BTreeMap<String, Observation>,
    }
    #[derive(Deserialize)]
    struct Observation {
        displaced: usize,
        words: Vec<String>,
    }
    let original: Historical = serde_json::from_slice(RETAINED).unwrap();
    assert_eq!(original.words, 1_983);
    assert_eq!(original.locales.len(), 194);
    assert_eq!(
        original
            .locales
            .values()
            .filter(|entry| entry.displaced == 11)
            .count(),
        193
    );
    assert_eq!(
        original
            .locales
            .values()
            .filter(|entry| entry.displaced == 13)
            .count(),
        1
    );
    let en: Vec<_> = original.locales["en_US"]
        .words
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        en,
        [
            "1-2A",
            "c.121G>A",
            "file(1).txt",
            "¼",
            "¼1",
            "¼a",
            "¼¼",
            "½",
            "½1",
            "½a",
            "½½",
        ]
    );
    let tk: Vec<_> = original.locales["tk_TM"]
        .words
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tk,
        [
            "-ña_",
            "1-2A",
            "c.121G>A",
            "file(1).txt",
            "¼",
            "¼1",
            "¼a",
            "¼¼",
            "½",
            "½1",
            "½a",
            "½½",
            "ñ-a-",
        ]
    );
    let outcomes = original
        .locales
        .into_iter()
        .map(|(name, entry)| {
            // Every historical list is complete, so none requires a lost tail.
            assert_eq!(entry.displaced, entry.words.len());
            assert!(entry.displaced < 40);
            (
                name,
                PunctuationOutcome::Compared(OrderDifference {
                    equal: false,
                    displaced: entry.words,
                }),
            )
        })
        .collect();
    let report = Report::new(original.words, outcomes);
    // Checks all locale names, word bytes, counts, field order and formatting.
    assert_eq!(report.json().unwrap(), RETAINED);
    assert_eq!(
        report.summary_line(),
        "194 locales, 1983 words; locales by displaced words: 11: 193, 13: 1"
    );
}

#[test]
fn report_preserves_unusual_names_and_supplied_error_strings_exactly() {
    let mut outcomes = BTreeMap::new();
    outcomes.insert("z\né".into(), compared(&[], false));
    outcomes.insert(
        "a\"name".into(),
        PunctuationOutcome::Failed([
            "fastmash: \"bad\"\nfr_é�".into(),
            "datamash: 'bad'\nréférence".into(),
        ]),
    );
    let report = Report::new(8, outcomes);
    let expected = r#"{
 "words": 8,
 "locales": {
  "a\"name": {
   "error": [
    "fastmash: \"bad\"\nfr_é�",
    "datamash: 'bad'\nréférence"
   ]
  },
  "z\né": {
   "displaced": 0,
   "words": []
  }
 }
}
"#;
    assert_eq!(report.json().unwrap(), expected.as_bytes());
    assert_eq!(
        report.summary_line(),
        "2 locales, 8 words; locales by displaced words: error: 1, 0: 1"
    );
}

#[test]
fn report_keeps_full_47_displaced_count_and_only_first_forty_words() {
    let moved: Vec<_> = (0..47).map(|index| format!("word-{index:02}")).collect();
    let outcomes = BTreeMap::from([(
        "many".into(),
        PunctuationOutcome::Compared(OrderDifference {
            equal: false,
            displaced: moved,
        }),
    )]);
    let report = Report::new(47, outcomes);
    let actual: Value = serde_json::from_slice(&report.json().unwrap()).unwrap();
    assert_eq!(actual["locales"]["many"]["displaced"], 47);
    assert_eq!(
        actual["locales"]["many"]["words"],
        json!([
            "word-00", "word-01", "word-02", "word-03", "word-04", "word-05", "word-06", "word-07",
            "word-08", "word-09", "word-10", "word-11", "word-12", "word-13", "word-14", "word-15",
            "word-16", "word-17", "word-18", "word-19", "word-20", "word-21", "word-22", "word-23",
            "word-24", "word-25", "word-26", "word-27", "word-28", "word-29", "word-30", "word-31",
            "word-32", "word-33", "word-34", "word-35", "word-36", "word-37", "word-38", "word-39",
        ])
    );
    assert_eq!(
        report.summary_line(),
        "1 locales, 47 words; locales by displaced words: 47: 1"
    );
}

#[test]
fn histogram_places_errors_before_lexically_ordered_numeric_buckets() {
    let ten = ["00", "01", "02", "03", "04", "05", "06", "07", "08", "09"];
    let outcomes = BTreeMap::from([
        ("a-two".into(), compared(&["a", "b"], false)),
        ("b-ten".into(), compared(&ten, false)),
        (
            "c-error".into(),
            PunctuationOutcome::Failed([String::new(), String::new()]),
        ),
        ("d-ten".into(), compared(&ten, false)),
    ]);
    let report = Report::new(99, outcomes);
    assert_eq!(
        report.summary_line(),
        "4 locales, 99 words; locales by displaced words: error: 1, 10: 2, 2: 1"
    );
    let actual: Value = serde_json::from_slice(&report.json().unwrap()).unwrap();
    assert_eq!(actual["locales"]["c-error"], json!({"error": ["", ""]}));
}

#[test]
fn empty_reports_and_unequal_empty_displacements_keep_original_wording_and_shape() {
    let empty = Report::new(0, BTreeMap::new());
    assert_eq!(
        empty.json().unwrap(),
        b"{\n \"words\": 0,\n \"locales\": {}\n}\n"
    );
    assert_eq!(
        empty.summary_line(),
        "0 locales, 0 words; locales by displaced words: "
    );
    let report = Report::new(
        3,
        BTreeMap::from([("unequal".into(), compared(&[], false))]),
    );
    let expected = b"{\n \"words\": 3,\n \"locales\": {\n  \"unequal\": {\n   \"displaced\": 0,\n   \"words\": []\n  }\n }\n}\n";
    assert_eq!(report.json().unwrap(), expected);
    assert_eq!(
        report.summary_line(),
        "1 locales, 3 words; locales by displaced words: 0: 1"
    );
}
