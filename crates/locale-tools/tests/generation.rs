#![forbid(unsafe_code)]

use fastmash_locale_tools::{
    CompiledLocale, Locale, Numeric, Reference, Snapshot, collect, parse_numeric,
};
use serde_json::json;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// This is deliberately an exact emitter fixture, compiled by the ordinary
// test consumer. Formatting it would change the historical emission bytes.
#[rustfmt::skip]
#[path = "fixtures/generated_numeric.rs"]
mod generated;

const UNUSUAL: [&str; 6] = [
    "quote\"name",
    "back\\slash",
    "carriage\rreturn",
    "line\nbreak",
    "fr_é",
    "bad\");compile_error!(\"injected",
];

fn reference() -> Reference {
    Reference {
        glibc: "fixture glibc 2.43".into(),
        machine: "fixture-host".into(),
    }
}

fn numeric(decimal: &str, thousands: &str, grouping: &[i64]) -> Numeric {
    Numeric {
        decimal_point: decimal.into(),
        thousands_sep: thousands.into(),
        grouping: grouping.to_vec(),
    }
}

fn original_profiles() -> Vec<(String, Numeric)> {
    vec![
        ("sr_RS@latin".into(), numeric(",", ".", &[3, -1])),
        ("nan_TW.UTF-8@latin".into(), numeric(".", "\u{a0}", &[3, 0])),
        ("de_DE.UTF-8".into(), numeric(",", ".", &[3, 3, 0])),
        ("fa_IR.UTF-8".into(), numeric("٫", ",", &[3, 0])),
        ("aa_ER".into(), numeric(".", "", &[])),
        ("C.UTF-8".into(), numeric(".", "", &[])),
    ]
}

fn generate(profiles: &[(String, Numeric)]) -> Snapshot {
    collect(
        reference(),
        profiles.iter().map(|(name, _)| name.clone()).collect(),
        |name| {
            Ok(profiles
                .iter()
                .find(|(source, _)| source == name)
                .expect("query must use an exact source-directory name")
                .1
                .clone())
        },
    )
    .unwrap()
}

fn compiled(name: &str, decimal: u8, thousands: &[u8], grouping: &[u8]) -> CompiledLocale {
    CompiledLocale {
        name: name.as_bytes().to_vec(),
        decimal,
        thousands: thousands.to_vec(),
        grouping: grouping.to_vec(),
    }
}

fn bytes(names: &[&str]) -> Vec<Vec<u8>> {
    names.iter().map(|name| name.as_bytes().to_vec()).collect()
}

#[test]
fn six_unusual_names_remain_data_in_all_three_tables() {
    for name in UNUSUAL {
        let refused = format!("{name}@refused");
        let snapshot = generate(&[
            (refused.clone(), numeric("٫", ",", &[3, 0])),
            (name.into(), numeric(".", "", &[])),
        ]);
        assert_eq!(
            snapshot.locales,
            vec![
                Locale {
                    name: name.into(),
                    numeric: numeric(".", "", &[])
                },
                Locale {
                    name: refused.clone(),
                    numeric: numeric("٫", ",", &[3, 0])
                },
            ]
        );
        assert_eq!(snapshot.bare_utf8, [name.to_owned(), refused.clone()]);
        let tables = snapshot.tables().unwrap();
        assert_eq!(tables.numeric, [compiled(name, b'.', b"", b"")]);
        assert_eq!(tables.bare_utf8, bytes(&[name]));
        assert_eq!(tables.refused, bytes(&[&refused]));
        let raw: serde_json::Value = serde_json::from_slice(&snapshot.json().unwrap()).unwrap();
        assert_eq!(
            raw,
            json!({
                "glibc": "fixture glibc 2.43", "machine": "fixture-host",
                "bare_utf8": [name, &refused],
                "locales": {
                    (name): {"decimal_point": ".", "thousands_sep": "", "grouping": []},
                    (refused.as_str()): {"decimal_point": "٫", "thousands_sep": ",", "grouping": [3, 0]},
                },
            })
        );
    }
}

#[test]
fn original_numeric_modifier_fixture_keeps_membership_order_and_bytes() {
    let snapshot = generate(&original_profiles());
    assert_eq!(
        snapshot.locales,
        vec![
            Locale {
                name: "aa_ER".into(),
                numeric: numeric(".", "", &[])
            },
            Locale {
                name: "de_DE".into(),
                numeric: numeric(",", ".", &[3, 3, 0])
            },
            Locale {
                name: "fa_IR".into(),
                numeric: numeric("٫", ",", &[3, 0])
            },
            Locale {
                name: "nan_TW@latin".into(),
                numeric: numeric(".", "\u{a0}", &[3, 0])
            },
            Locale {
                name: "sr_RS@latin".into(),
                numeric: numeric(",", ".", &[3, -1])
            },
        ]
    );
    assert_eq!(snapshot.bare_utf8, ["aa_ER", "sr_RS@latin"]);
    let tables = snapshot.tables().unwrap();
    assert_eq!(
        tables.numeric,
        vec![
            compiled("aa_ER", b'.', b"", b""),
            compiled("de_DE", b',', b".", b"\x03\x03\x00"),
            compiled("nan_TW@latin", b'.', b"\xc2\xa0", b"\x03\x00"),
            compiled("sr_RS@latin", b',', b".", b"\x03\x7f"),
        ]
    );
    assert_eq!(tables.bare_utf8, bytes(&["aa_ER", "sr_RS@latin"]));
    assert_eq!(tables.refused, bytes(&["fa_IR"]));
    let raw: serde_json::Value = serde_json::from_slice(&snapshot.json().unwrap()).unwrap();
    assert_eq!(
        raw,
        json!({
            "glibc": "fixture glibc 2.43", "machine": "fixture-host",
            "bare_utf8": ["aa_ER", "sr_RS@latin"],
            "locales": {
                "aa_ER": {"decimal_point": ".", "thousands_sep": "", "grouping": []},
                "de_DE": {"decimal_point": ",", "thousands_sep": ".", "grouping": [3, 3, 0]},
                "fa_IR": {"decimal_point": "٫", "thousands_sep": ",", "grouping": [3, 0]},
                "nan_TW@latin": {"decimal_point": ".", "thousands_sep": "\u{a0}", "grouping": [3, 0]},
                "sr_RS@latin": {"decimal_point": ",", "thousands_sep": ".", "grouping": [3, -1]},
            },
        })
    );
}

#[test]
fn sorted_queries_replace_all_codesets_and_keep_first_key_position_last_value() {
    let mut queried = Vec::new();
    let snapshot = collect(
        reference(),
        vec![
            "az.UTF-8".into(),
            "a.UTF-8z".into(),
            "aa.UTF-8".into(),
            "a.UTF-8y".into(),
            "C.other".into(),
            "C.UTF-8".into(),
            "a.UTF-8.UTF-8z".into(),
        ],
        |name| {
            queried.push(name.to_owned());
            Ok(if name == "az.UTF-8" {
                numeric(",", "last", &[3, -1])
            } else {
                numeric(".", name, &[])
            })
        },
    )
    .unwrap();
    assert_eq!(
        queried,
        [
            "a.UTF-8.UTF-8z",
            "a.UTF-8y",
            "a.UTF-8z",
            "aa.UTF-8",
            "az.UTF-8"
        ]
    );
    assert_eq!(
        snapshot.locales,
        vec![
            Locale {
                name: "az".into(),
                numeric: numeric(",", "last", &[3, -1])
            },
            Locale {
                name: "ay".into(),
                numeric: numeric(".", "a.UTF-8y", &[])
            },
            Locale {
                name: "aa".into(),
                numeric: numeric(".", "aa.UTF-8", &[])
            },
        ]
    );
    assert!(snapshot.bare_utf8.is_empty());
    assert_eq!(
        snapshot
            .tables()
            .unwrap()
            .numeric
            .iter()
            .map(|locale| locale.name.clone())
            .collect::<Vec<_>>(),
        bytes(&["aa", "ay", "az"])
    );
    let raw = String::from_utf8(snapshot.json().unwrap()).unwrap();
    let positions = ["\"az\":", "\"ay\":", "\"aa\":"].map(|key| raw.find(key).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn emitted_fixture_is_exact_source_and_real_profile_data() {
    let mut profiles = original_profiles();
    for name in UNUSUAL {
        profiles.push((name.into(), numeric(".", "", &[])));
        profiles.push((format!("{name}@refused"), numeric("٫", ",", &[3, 0])));
    }
    let snapshot = generate(&profiles);
    let expected = vec![
        compiled("aa_ER", b'.', b"", b""),
        compiled("back\\slash", b'.', b"", b""),
        compiled("bad\");compile_error!(\"injected", b'.', b"", b""),
        compiled("carriage\rreturn", b'.', b"", b""),
        compiled("de_DE", b',', b".", b"\x03\x03\x00"),
        compiled("fr_é", b'.', b"", b""),
        compiled("line\nbreak", b'.', b"", b""),
        compiled("nan_TW@latin", b'.', b"\xc2\xa0", b"\x03\x00"),
        compiled("quote\"name", b'.', b"", b""),
        compiled("sr_RS@latin", b',', b".", b"\x03\x7f"),
    ];
    let tables = snapshot.tables().unwrap();
    assert_eq!(tables.numeric, expected);
    let actual: Vec<_> = generated::NUMERIC
        .iter()
        .map(|(name, profile)| CompiledLocale {
            name: name.to_vec(),
            decimal: profile.radix(),
            thousands: profile.thousands().to_vec(),
            grouping: profile.grouping().to_vec(),
        })
        .collect();
    assert_eq!(actual, expected);
    let bare = bytes(&[
        "aa_ER",
        "back\\slash",
        "bad\");compile_error!(\"injected",
        "carriage\rreturn",
        "fr_é",
        "line\nbreak",
        "quote\"name",
        "sr_RS@latin",
    ]);
    let refused = bytes(&[
        "back\\slash@refused",
        "bad\");compile_error!(\"injected@refused",
        "carriage\rreturn@refused",
        "fa_IR",
        "fr_é@refused",
        "line\nbreak@refused",
        "quote\"name@refused",
    ]);
    assert_eq!(tables.bare_utf8, bare);
    assert_eq!(tables.refused, refused);
    assert_eq!(
        generated::BARE_UTF8
            .iter()
            .map(|name| name.to_vec())
            .collect::<Vec<_>>(),
        bare
    );
    assert_eq!(
        generated::REFUSED
            .iter()
            .map(|name| name.to_vec())
            .collect::<Vec<_>>(),
        refused
    );
    let output = snapshot.rust_source().unwrap();
    assert_eq!(output, include_str!("fixtures/generated_numeric.rs"));
    let json_output = snapshot.json().unwrap();
    let directory = FixtureDirectory::new();
    for (name, data) in [
        ("table.rs", output.as_bytes()),
        ("table.json", json_output.as_slice()),
    ] {
        let path = directory.0.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(data).unwrap();
        drop(file);
        assert_eq!(fs::read(path).unwrap(), data);
    }
}

#[test]
fn query_parser_retains_json_strings_grouping_and_last_assignments() {
    let reply = "decimal_point=\".\"\nunknown=ignored\ndecimal_point=\",\"\n\
thousands_sep=\"\\u00a0\"\ngrouping=3; -1;0\n";
    assert_eq!(
        parse_numeric(reply).unwrap(),
        numeric(",", "\u{a0}", &[3, -1, 0])
    );
    assert_eq!(
        parse_numeric("decimal_point=\".\"\nthousands_sep=\"\"\ngrouping=\n").unwrap(),
        numeric(".", "", &[])
    );
    assert!(parse_numeric("decimal_point=\".\"\ngrouping=3\n").is_err());
    assert!(parse_numeric("decimal_point=\".\"\nthousands_sep=\"\"\ngrouping=bad\n").is_err());
}

#[test]
fn empty_and_refused_tables_remain_valid_emission_and_query_errors_propagate() {
    let empty = generate(&[]);
    assert!(empty.tables().unwrap().numeric.is_empty());
    assert!(
        empty
            .rust_source()
            .unwrap()
            .contains("NUMERIC: &[(&[u8], Profile)] = &[\n\n];")
    );
    let refused = generate(&[("only".into(), numeric("٫", "", &[]))]);
    assert!(refused.tables().unwrap().numeric.is_empty());
    assert!(refused.tables().unwrap().bare_utf8.is_empty());
    assert_eq!(refused.tables().unwrap().refused, bytes(&["only"]));
    assert!(
        collect(reference(), vec!["broken".into()], |_| Err(
            "query failed".into()
        ))
        .is_err()
    );
    for size in [-2, 256] {
        let unsupported = generate(&[("invalid".into(), numeric(".", "", &[size]))]);
        assert!(unsupported.rust_source().is_err());
    }
    let mut multiline_reference = empty;
    multiline_reference.reference.glibc = "fixture\ncompile_error!(\"not a version line\");".into();
    assert!(multiline_reference.rust_source().is_err());
}

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-locale-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
