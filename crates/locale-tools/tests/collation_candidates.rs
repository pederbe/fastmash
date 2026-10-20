#![forbid(unsafe_code)]

use fastmash_locale_tools::collation_candidates::{Exemplars, NumericLocales};

const NUMERIC: &[u8] = br#"{"glibc":"reference glibc","locales":{"z_ZZ@variant":{},"a_AA":{}}}"#;
const EXEMPLARS: &[u8] = br#"{"icu":"78","locales":{"a_AA":{"alphabet":["a","ch"],"auxiliary":[]},"z_ZZ@variant":{"alphabet":null,"auxiliary":null}}}"#;

#[test]
fn projection_preserves_names_alphabets_order_and_uppercase_tags() {
    let numeric = NumericLocales::parse(NUMERIC).unwrap();
    let exemplars = Exemplars::parse(EXEMPLARS, &numeric).unwrap();
    let mut visited = Vec::new();
    let result = exemplars
        .project(numeric, |name| {
            visited.push(name.to_owned());
            Ok(name == "a_AA")
        })
        .unwrap();
    assert_eq!(visited, ["a_AA", "z_ZZ@variant"]);
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"icu":"78","glibc":"reference glibc","locales":{
            "a_AA":{"tags":["a-AA-u-kf-upper","und-u-kf-upper"],"alphabet":["a","ch"],"auxiliary":[]},
            "z_ZZ@variant":{"tags":["z-ZZ@variant","und"],"alphabet":null,"auxiliary":null}
        }})
    );
}

#[test]
fn wrong_reference_missing_fields_and_mismatched_names_refuse() {
    let numeric = NumericLocales::parse(NUMERIC).unwrap();
    let raw = std::str::from_utf8(EXEMPLARS).unwrap();
    for invalid in [
        raw.replace("\"78\"", "\"77\""),
        raw.replace("z_ZZ@variant", "different"),
        raw.replace("\"auxiliary\":[]", "\"extra\":[]"),
        raw.replace("\"auxiliary\":[]", "\"auxiliary\":[],\"extra\":1"),
        raw.replace("\"alphabet\":[\"a\",\"ch\"]", "\"alphabet\":[1]"),
    ] {
        assert!(
            Exemplars::parse(invalid.as_bytes(), &numeric).is_err(),
            "{invalid}"
        );
    }
    for invalid in [
        b"{}".as_slice(),
        br#"{"glibc":"x","locales":{}}"#,
        br#"{"glibc":"x","locales":{"a\u0000b":{}}}"#,
    ] {
        assert!(NumericLocales::parse(invalid).is_err());
    }
}

#[test]
fn failed_probe_stops_projection() {
    let numeric = NumericLocales::parse(NUMERIC).unwrap();
    let exemplars = Exemplars::parse(EXEMPLARS, &numeric).unwrap();
    let mut calls = 0;
    assert!(
        exemplars
            .project(numeric, |_| {
                calls += 1;
                Err("probe failed".into())
            })
            .is_err()
    );
    assert_eq!(calls, 1);
}
