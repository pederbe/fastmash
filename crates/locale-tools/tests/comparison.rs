#![forbid(unsafe_code)]

use fastmash_locale_tools::comparison::{Plan, normalize_stderr};

const REFERENCE: &[u8] = include_bytes!("../../../data/locales/glibc-lc-numeric.json");
const EACH_DOT: &[u8] = b"0\t0\n1\t1\n-1\t2\n12\t3\n123\t4\n1234\t5\n12345\t6\n123456\t7\n1234567\t8\n12345678\t9\n123456789\t10\n1234567890\t11\n98765432109876\t12\n-4321987.5\t13\n0.125\t14\n1e20\t15\n-2.5e-7\t16\n999999.995\t17\n1000000\t18\n-0.5\t19\n123456789012345678901234567890\t20\n";
const EACH_COMMA: &[u8] = b"0\t0\n1\t1\n-1\t2\n12\t3\n123\t4\n1234\t5\n12345\t6\n123456\t7\n1234567\t8\n12345678\t9\n123456789\t10\n1234567890\t11\n98765432109876\t12\n-4321987,5\t13\n0,125\t14\n1e20\t15\n-2,5e-7\t16\n999999,995\t17\n1000000\t18\n-0,5\t19\n123456789012345678901234567890\t20\n";
const COLUMN_DOT: &[u8] = b"0\n1\n-1\n12\n123\n1234\n12345\n123456\n1234567\n12345678\n123456789\n1234567890\n98765432109876\n-4321987.5\n0.125\n1e20\n-2.5e-7\n999999.995\n1000000\n-0.5\n";
const COLUMN_COMMA: &[u8] = b"0\n1\n-1\n12\n123\n1234\n12345\n123456\n1234567\n12345678\n123456789\n1234567890\n98765432109876\n-4321987,5\n0,125\n1e20\n-2,5e-7\n999999,995\n1000000\n-0,5\n";

#[test]
fn representatives_keep_exact_raw_triples_and_first_sorted_names() {
    let plan = Plan::from_json(REFERENCE, false).unwrap();
    let expected: &[(&str, &str, &str, &str, &[i64])] = &[
        ("aa_DJ", "aa_DJ.UTF-8", ".", "", &[-1]),
        ("aa_ET", "aa_ET", ".", ",", &[3]),
        ("agr_PE", "agr_PE", ",", ".", &[3]),
        ("as_IN", "as_IN", ".", ",", &[3, 2]),
        ("bg_BG", "bg_BG.UTF-8", ",", "", &[3]),
        ("br_FR", "br_FR.UTF-8", ",", "\u{202f}", &[3]),
        ("bs_BA", "bs_BA.UTF-8", ",", "", &[-1]),
        ("cmn_TW", "cmn_TW", ".", ",", &[4]),
        ("de_CH", "de_CH.UTF-8", ".", "\u{2019}", &[3]),
        ("el_CY", "el_CY.UTF-8", ",", ".", &[-1]),
        ("eo_US", "eo_US.UTF-8", ".", ",", &[3, 3]),
        ("es_MX", "es_MX.UTF-8", ".", "\u{202f}", &[3]),
        ("rif_MA", "rif_MA", ".", "", &[3]),
        ("tok", "tok", ".", "\u{202f}", &[-1]),
        ("unm_US", "unm_US", ".", "\u{202f}", &[2, 2, 2, 3]),
    ];
    let actual: Vec<_> = plan
        .locales
        .iter()
        .map(|locale| {
            (
                locale.name.as_str(),
                locale.locale.as_str(),
                locale.rules.decimal_point.as_str(),
                locale.rules.thousands_sep.as_str(),
                locale.rules.grouping.as_slice(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(plan.comparisons(), 315);
}

#[test]
fn all_keeps_admitted_names_and_historical_modifiers_without_correcting_them() {
    let plan = Plan::from_json(REFERENCE, true).unwrap();
    assert_eq!(plan.locales.len(), 352);
    assert_eq!(plan.comparisons(), 7392);
    assert!(
        plan.locales
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
    );
    assert!(!plan.locales.iter().any(|locale| locale.name == "ps_AF"));
    let raw: serde_json::Value = serde_json::from_slice(REFERENCE).unwrap();
    assert_eq!(raw["locales"].as_object().unwrap().len(), 353);
    assert_eq!(
        raw["locales"]["ps_AF"],
        serde_json::json!({
            "decimal_point": "\u{066b}", "thousands_sep": "\u{066c}", "grouping": [3],
        })
    );
    assert!(
        raw["bare_utf8"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "ps_AF")
    );
    let modifiers: Vec<_> = plan
        .locales
        .iter()
        .filter(|locale| locale.name.contains('@'))
        .map(|locale| (locale.name.as_str(), locale.locale.as_str()))
        .collect();
    assert_eq!(
        modifiers,
        [
            ("be_BY@latin", "be_BY@latin.UTF-8"),
            ("br_FR@euro", "br_FR@euro.UTF-8"),
            ("ca_ES@euro", "ca_ES@euro.UTF-8"),
            ("ca_ES@valencia", "ca_ES@valencia.UTF-8"),
            ("de_AT@euro", "de_AT@euro.UTF-8"),
            ("de_BE@euro", "de_BE@euro.UTF-8"),
            ("de_DE@euro", "de_DE@euro.UTF-8"),
            ("de_LU@euro", "de_LU@euro.UTF-8"),
            ("el_GR@euro", "el_GR@euro.UTF-8"),
            ("en_IE@euro", "en_IE@euro.UTF-8"),
            ("es_ES@euro", "es_ES@euro.UTF-8"),
            ("eu_ES@euro", "eu_ES@euro.UTF-8"),
            ("eu_FR@euro", "eu_FR@euro.UTF-8"),
            ("fi_FI@euro", "fi_FI@euro.UTF-8"),
            ("fr_BE@euro", "fr_BE@euro.UTF-8"),
            ("fr_FR@euro", "fr_FR@euro.UTF-8"),
            ("fr_LU@euro", "fr_LU@euro.UTF-8"),
            ("ga_IE@euro", "ga_IE@euro.UTF-8"),
            ("gez_ER@abegede", "gez_ER@abegede.UTF-8"),
            ("gez_ET@abegede", "gez_ET@abegede.UTF-8"),
            ("gl_ES@euro", "gl_ES@euro.UTF-8"),
            ("it_IT@euro", "it_IT@euro.UTF-8"),
            ("ks_IN@devanagari", "ks_IN@devanagari.UTF-8"),
            ("nan_TW@latin", "nan_TW@latin.UTF-8"),
            ("nl_BE@euro", "nl_BE@euro.UTF-8"),
            ("nl_NL@euro", "nl_NL@euro.UTF-8"),
            ("pt_PT@euro", "pt_PT@euro.UTF-8"),
            ("sd_IN@devanagari", "sd_IN@devanagari.UTF-8"),
            ("sr_RS@latin", "sr_RS@latin.UTF-8"),
            ("sv_FI@euro", "sv_FI@euro.UTF-8"),
            ("tt_RU@iqtelif", "tt_RU@iqtelif.UTF-8"),
            ("ug_CN@latin", "ug_CN@latin.UTF-8"),
            ("uz_UZ@cyrillic", "uz_UZ@cyrillic.UTF-8"),
            ("wa_BE@euro", "wa_BE@euro.UTF-8"),
        ]
    );
    let base = plan
        .locales
        .iter()
        .find(|locale| locale.name == "nan_TW")
        .unwrap();
    let modifier = plan
        .locales
        .iter()
        .find(|locale| locale.name == "nan_TW@latin")
        .unwrap();
    assert_eq!(base.rules.grouping, [4]);
    assert_eq!(base.locale, "nan_TW");
    assert_eq!(modifier.rules.grouping, [3]);
    assert_eq!(modifier.locale, "nan_TW@latin.UTF-8");
}

#[test]
fn cases_keep_all_21_arguments_and_exact_localized_payloads_in_locale_order() {
    let expected_arguments: &[&[&str]] = &[
        &["-g", "2", "sum", "1"],
        &["--format", "%'.2f", "-g", "2", "sum", "1"],
        &["--format", "%'14.2f", "-g", "2", "sum", "1"],
        &["--format", "%'014.2f", "-g", "2", "sum", "1"],
        &["--format", "%-'16.3f", "-g", "2", "sum", "1"],
        &["--format", "%+'.0f", "-g", "2", "sum", "1"],
        &["--format", "%' 025.3f", "-g", "2", "sum", "1"],
        &["--format", "%'Lf", "-g", "2", "sum", "1"],
        &["--format", "%'g", "-g", "2", "sum", "1"],
        &["--format", "%'.10g", "-g", "2", "sum", "1"],
        &["--format", "%'.20g", "-g", "2", "sum", "1"],
        &["--format", "%'e", "-g", "2", "sum", "1"],
        &["--format", "%'a", "-g", "2", "sum", "1"],
        &["--format", "%.3f", "-g", "2", "sum", "1"],
        &["--format", "%g", "-g", "2", "sum", "1"],
        &["--format", "%'20.4f", "-g", "2", "sum", "1"],
        &["--format", "%'.0f", "-g", "2", "sum", "1"],
        &["sum", "1", "mean", "1"],
        &["trimmean:0.2", "1"],
        &["perc:90", "1"],
        &["--format", "%'.3f", "sum", "1"],
    ];
    let plan = Plan::from_json(REFERENCE, false).unwrap();
    for (name, each, column, trim) in [
        ("aa_ET", EACH_DOT, COLUMN_DOT, "trimmean:0.2"),
        ("agr_PE", EACH_COMMA, COLUMN_COMMA, "trimmean:0,2"),
    ] {
        let locale = plan
            .locales
            .iter()
            .find(|locale| locale.name == name)
            .unwrap();
        let cases: Vec<_> = locale.cases().collect();
        assert_eq!(cases.len(), 21);
        for (index, case) in cases.iter().enumerate() {
            let expected = if index == 18 {
                &[trim, "1"][..]
            } else {
                expected_arguments[index]
            };
            let arguments: Vec<_> = case.arguments.iter().map(String::as_str).collect();
            assert_eq!(arguments, expected, "{name} case {index}");
            assert_eq!(
                case.input,
                if index < 17 { each } else { column },
                "{name} case {index}"
            );
        }
        assert_eq!(cases[0].input.len(), 223);
        assert_eq!(cases[17].input.len(), 139);
    }
    let indian = plan
        .locales
        .iter()
        .find(|locale| locale.name == "as_IN")
        .unwrap();
    let space_format = indian.cases().nth(6).unwrap();
    assert_eq!(space_format.arguments[1], "%' 025.3f");
    assert_eq!(space_format.input, EACH_DOT);
    let breton = plan
        .locales
        .iter()
        .find(|locale| locale.name == "br_FR")
        .unwrap();
    let long_format = breton.cases().nth(7).unwrap();
    assert_eq!(long_format.arguments[1], "%'Lf");
    assert_eq!(long_format.input, EACH_COMMA);
    assert!(long_format.input.is_ascii());
    let order: Vec<_> = plan
        .locales
        .iter()
        .flat_map(|locale| {
            locale
                .cases()
                .enumerate()
                .map(move |(index, _)| (locale.name.as_str(), index))
        })
        .take(23)
        .collect();
    assert_eq!(&order[20..], &[("aa_DJ", 20), ("aa_ET", 0), ("aa_ET", 1)]);
}

#[test]
fn stderr_normalization_uses_ascii_byte_words_without_decoding() {
    let fixtures: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (b"fastmash", b"PROG"),
        (b"datamash: error\n", b"PROG: error\n"),
        (b"/usr/bin/fastmash: error\n", b"/usr/bin/PROG: error\n"),
        (b"fastmash-datamash", b"PROG-PROG"),
        (b"fastmash.datamash", b"PROG.PROG"),
        (b"'fastmash' \"datamash\"", b"'PROG' \"PROG\""),
        (b"xfastmash fastmash2", b"xfastmash fastmash2"),
        (b"1datamash datamash9", b"1datamash datamash9"),
        (
            b"_datamash_ x_fastmash fastmash_x",
            b"_datamash_ x_fastmash fastmash_x",
        ),
        (b"fastmashdatamash", b"fastmashdatamash"),
        (b"FASTMASH DataMash", b"FASTMASH DataMash"),
        (b"fastmash\0datamash", b"PROG\0PROG"),
        (b"\xfffastmash\x80", b"\xffPROG\x80"),
        (b"\xc3\xa9fastmash\xc3\xa9", b"\xc3\xa9PROG\xc3\xa9"),
        (b"PROG fastmash PROG", b"PROG PROG PROG"),
        (b"fastmash:datamash:fastmash", b"PROG:PROG:PROG"),
        (
            b"fastmash-fastmashfastmash-datamash",
            b"PROG-fastmashfastmash-PROG",
        ),
    ];
    for (raw, expected) in fixtures {
        assert_eq!(normalize_stderr(raw), *expected, "raw bytes: {raw:?}");
    }
}
