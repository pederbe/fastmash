#![forbid(unsafe_code)]

#[test]
fn retained_independent_icu_alphabets_match_or_unsupported_version_refuses() {
    // This retained file comes from the original installed ICU producer, not
    // Fastmash's collation results. Tags are independently checked by GNU sort.
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../data/locales/collation-candidates.json"
    ))
    .unwrap();
    let expected = source["locales"].as_object().unwrap();
    let result = fastmash_icu_exemplars::collect(expected.keys().cloned());
    if !fastmash_icu_exemplars::supported() {
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<fastmash_icu_exemplars::Refusal>()
                .is_some()
        );
        return;
    }
    let actual = serde_json::to_value(result.unwrap()).unwrap();
    assert_eq!(actual["icu"], source["icu"]);
    assert_eq!(actual["locales"].as_object().unwrap().len(), expected.len());
    for (name, expected) in expected {
        assert_eq!(
            actual["locales"][name]["alphabet"], expected["alphabet"],
            "{name} standard"
        );
        assert_eq!(
            actual["locales"][name]["auxiliary"], expected["auxiliary"],
            "{name} auxiliary"
        );
    }
}
