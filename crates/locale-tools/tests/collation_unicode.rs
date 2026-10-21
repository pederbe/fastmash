#![forbid(unsafe_code)]

use fastmash_locale_tools::collation_unicode::{BLOCKS, alphabet_words, block_words};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Candidates {
    locales: BTreeMap<String, Exemplars>,
}

#[derive(Deserialize)]
struct Exemplars {
    alphabet: Option<Vec<String>>,
    auxiliary: Option<Vec<String>>,
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn input_bytes(words: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in words {
        bytes.extend_from_slice(word.as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

fn contains(words: &[String], expected: &str) -> bool {
    words.iter().any(|word| word == expected)
}

#[test]
fn complete_alphabet_vectors_match_the_independent_unicode15_producer() {
    let cases: &[(&str, &[u8])] = &[
        (
            include_str!("fixtures/unicode15/mixed.json"),
            include_bytes!("fixtures/unicode15/alphabet-mixed.stdin"),
        ),
        (
            include_str!("fixtures/unicode15/over120.json"),
            include_bytes!("fixtures/unicode15/alphabet-over120.stdin"),
        ),
    ];
    for &(json, expected) in cases {
        let candidates: Candidates = serde_json::from_str(json).unwrap();
        assert_eq!(candidates.locales.len(), 1);
        let (name, entry) = candidates.locales.into_iter().next().unwrap();
        let words = alphabet_words(
            entry.alphabet.as_deref(),
            entry.auxiliary.as_deref(),
            name.split('_').next().unwrap(),
        );
        assert_eq!(input_bytes(&words).as_slice(), expected);
    }
}

#[test]
fn all_29_complete_block_vectors_match_in_retained_order() {
    let cases: &[(&str, &[u8])] = &[
        ("latin", include_bytes!("fixtures/unicode15/latin.stdin")),
        (
            "vietnamese",
            include_bytes!("fixtures/unicode15/vietnamese.stdin"),
        ),
        ("greek", include_bytes!("fixtures/unicode15/greek.stdin")),
        (
            "cyrillic",
            include_bytes!("fixtures/unicode15/cyrillic.stdin"),
        ),
        (
            "armenian",
            include_bytes!("fixtures/unicode15/armenian.stdin"),
        ),
        ("hebrew", include_bytes!("fixtures/unicode15/hebrew.stdin")),
        ("arabic", include_bytes!("fixtures/unicode15/arabic.stdin")),
        (
            "devanagari",
            include_bytes!("fixtures/unicode15/devanagari.stdin"),
        ),
        (
            "bengali",
            include_bytes!("fixtures/unicode15/bengali.stdin"),
        ),
        (
            "gurmukhi",
            include_bytes!("fixtures/unicode15/gurmukhi.stdin"),
        ),
        (
            "gujarati",
            include_bytes!("fixtures/unicode15/gujarati.stdin"),
        ),
        ("oriya", include_bytes!("fixtures/unicode15/oriya.stdin")),
        ("tamil", include_bytes!("fixtures/unicode15/tamil.stdin")),
        ("telugu", include_bytes!("fixtures/unicode15/telugu.stdin")),
        (
            "kannada",
            include_bytes!("fixtures/unicode15/kannada.stdin"),
        ),
        (
            "malayalam",
            include_bytes!("fixtures/unicode15/malayalam.stdin"),
        ),
        (
            "sinhala",
            include_bytes!("fixtures/unicode15/sinhala.stdin"),
        ),
        ("thai", include_bytes!("fixtures/unicode15/thai.stdin")),
        ("lao", include_bytes!("fixtures/unicode15/lao.stdin")),
        (
            "tibetan",
            include_bytes!("fixtures/unicode15/tibetan.stdin"),
        ),
        (
            "myanmar",
            include_bytes!("fixtures/unicode15/myanmar.stdin"),
        ),
        (
            "georgian",
            include_bytes!("fixtures/unicode15/georgian.stdin"),
        ),
        (
            "ethiopic",
            include_bytes!("fixtures/unicode15/ethiopic.stdin"),
        ),
        ("khmer", include_bytes!("fixtures/unicode15/khmer.stdin")),
        ("thaana", include_bytes!("fixtures/unicode15/thaana.stdin")),
        (
            "hiragana",
            include_bytes!("fixtures/unicode15/hiragana.stdin"),
        ),
        (
            "katakana",
            include_bytes!("fixtures/unicode15/katakana.stdin"),
        ),
        ("hangul", include_bytes!("fixtures/unicode15/hangul.stdin")),
        ("han", include_bytes!("fixtures/unicode15/han.stdin")),
    ];
    assert_eq!(BLOCKS.len(), cases.len());
    for (block, &(name, expected)) in BLOCKS.iter().zip(cases) {
        assert_eq!(block.name, name);
        assert_eq!(
            input_bytes(&block_words(name).unwrap()).as_slice(),
            expected,
            "{name}"
        );
    }
}

#[test]
fn missing_empty_and_null_exemplars_agree_but_empty_strings_remain() {
    let nulls: Exemplars = serde_json::from_str(r#"{"alphabet":null,"auxiliary":null}"#).unwrap();
    let baseline = alphabet_words(None, None, "en");
    assert_eq!(baseline, alphabet_words(Some(&[]), Some(&[]), "en"));
    assert_eq!(
        baseline,
        alphabet_words(nulls.alphabet.as_deref(), nulls.auxiliary.as_deref(), "en")
    );
    assert!(!contains(&baseline, ""));
    for (alphabet, auxiliary) in [(Some(strings(&[""])), None), (None, Some(strings(&[""])))] {
        let words = alphabet_words(alphabet.as_deref(), auxiliary.as_deref(), "en");
        assert!(contains(&words, ""));
    }
}

#[test]
fn pair_threshold_counts_only_unique_standard_strings_plus_ascii() {
    // 94 distinct non-ASCII-letter strings plus the 26 ASCII entries is 120.
    let mut alphabet: Vec<String> = (0..94).map(|index| format!("!{index}")).collect();
    alphabet.extend(strings(&["!0", "a"]));
    let auxiliary: Vec<String> = (0..150).map(|index| format!("?{index}")).collect();
    let at_120 = alphabet_words(Some(&alphabet), Some(&auxiliary), "cmn");
    assert!(contains(&at_120, "!0!1"));
    assert!(contains(&at_120, "\u{20064}"));
    alphabet.push("!94".to_owned());
    let at_121 = alphabet_words(Some(&alphabet), Some(&auxiliary), "cmn");
    assert!(!contains(&at_121, "!0!1"));
    assert!(contains(&at_121, "\u{20064}"));
}

#[test]
fn pairs_keep_literal_strings_and_filtering_does_not_normalize_them() {
    let alphabet = strings(&["", "a\u{301}", "!", "ch", "\0"]);
    let words = alphabet_words(Some(&alphabet), None, "en");
    for expected in [
        "",
        "a\u{301}!",
        "A\u{301}!",
        "ch!",
        "Ch!",
        "\0",
        "\0!",
        "!\0",
    ] {
        assert!(contains(&words, expected), "missing {expected:?}");
    }
    assert!(!contains(&words, "á!"));
    assert!(!contains(&words, "CH!"));
}

#[test]
fn filler_filters_each_full_transform_by_unicode15_letter_or_number_category() {
    let auxiliary = strings(&[
        "İ", "ﬃ", "ß", "ǳ", "ǅ", "Ⅻ", "²", "٥", "ʰ", "अ", "中", "\u{345}", "a\u{301}", "!", "\0",
        "\u{1c89}",
    ]);
    let words = alphabet_words(None, Some(&auxiliary), "en");
    for expected in [
        "İ", "İa", "aİ", "ﬃ", "FFI", "Ffi", "ß", "SS", "Ss", "ǳ", "Ǳ", "ǲ", "Ǆ", "ǅ", "ǆ", "Ⅻ",
        "ⅻ", "²", "٥", "ʰ", "अ", "中", "Ι",
    ] {
        assert!(contains(&words, expected), "missing {expected:?}");
    }
    // Lowercasing retains the ligature, while title and uppercase expand it.
    // Dotted-I's lowercase expansion contains a mark. U+1C89 is unassigned in
    // Unicode 15, even when a newer compiler recognizes it as a letter.
    for excluded in [
        "ffi",
        "i\u{307}",
        "i\u{307}a",
        "ai\u{307}",
        "a\u{301}",
        "\u{345}",
        "!",
        "\0",
        "\u{1c89}",
    ] {
        assert!(!contains(&words, excluded), "unexpected {excluded:?}");
    }
    // A Han character in auxiliary data alone does not trigger the additions.
    assert!(!contains(&words, "\u{20064}"));
    assert_eq!(words, alphabet_words(None, Some(&auxiliary), "tr"));
}

#[test]
fn unicode_fifteen_casing_retains_mixed_string_variants() {
    // Unicode 16 assigns new uppercase partners to these Unicode 15 letters.
    // The retained producer uppercases the ASCII suffix without changing them.
    let alphabet = strings(&["\u{19b}a", "\u{264}a"]);
    let words = alphabet_words(Some(&alphabet), None, "en");
    for expected in ["\u{19b}A", "\u{264}A"] {
        assert!(contains(&words, expected), "missing {expected:?}");
    }
    for excluded in ["\u{a7dc}A", "\u{a7cb}A"] {
        assert!(!contains(&words, excluded), "unexpected {excluded:?}");
    }
}

#[test]
fn whole_string_casing_expands_and_uses_original_sigma_context() {
    let auxiliary = strings(&["ΟΣ", "AΣ", "Σ", "AΣB", "AΣ1", "ΣA", "ΑΣΣ"]);
    let words = alphabet_words(None, Some(&auxiliary), "en");
    for expected in [
        "ος", "Ος", "aς", "Aς", "σ", "aσb", "Aσb", "aς1", "Aς1", "σa", "Σa", "ασς", "Ασς",
    ] {
        assert!(contains(&words, expected), "missing {expected:?}");
    }
    assert!(!contains(&words, "οσ"));

    // Empty-string pairing makes capitalize observable even when marks or
    // punctuation exclude the corresponding variants from the filler branch.
    let alphabet = strings(&[
        "",
        "A\u{301}Σ",
        "AΣ\u{301}B",
        "AΣ\u{301}!",
        "A\u{345}Σ",
        "\u{345}Σ",
        "AΣ\u{345}",
        "AΣ\u{345}B",
        "!ΟΣ",
        "ßΣ",
        "1ΟΣ",
    ]);
    let words = alphabet_words(Some(&alphabet), None, "en");
    // U+0345 is both Cased and Case_Ignorable. Its title expansion is Iota,
    // but sigma must still inspect the original ignorable head and neighbors.
    for expected in [
        "A\u{301}ς",
        "Aσ\u{301}b",
        "Aς\u{301}!",
        "A\u{345}ς",
        "Ισ",
        "Aς\u{345}",
        "Aσ\u{345}b",
        "!ος",
        "Ssς",
        "1ος",
    ] {
        assert!(contains(&words, expected), "missing {expected:?}");
    }
    for excluded in ["A\u{301}σ", "Ις", "!Ος", "1Ος"] {
        assert!(!contains(&words, excluded), "unexpected {excluded:?}");
    }
}

#[test]
fn han_language_trigger_is_exact_and_uses_only_the_three_short_ranges() {
    for language in ["cmn", "hak", "lzh", "nan"] {
        let words = alphabet_words(None, None, language);
        for expected in [
            "\u{3400}",
            "\u{3464}",
            "\u{20000}",
            "\u{20064}",
            "\u{f900}",
            "\u{f964}",
        ] {
            assert!(contains(&words, expected), "{language}: {expected:?}");
        }
        for excluded in ["\u{3465}", "\u{20065}", "\u{f965}"] {
            assert!(!contains(&words, excluded), "{language}: {excluded:?}");
        }
    }
    for language in ["", "zh", "zh_CN", "cmn_TW", "CMN", " cmn", "nan "] {
        assert!(
            !contains(&alphabet_words(None, None, language), "\u{20064}"),
            "{language:?}"
        );
    }
}

#[test]
fn han_name_trigger_uses_assigned_unicode15_endpoints_and_single_standard_scalars() {
    // Literal assigned endpoints from Unicode 15 DerivedName.txt, including H.
    for scalar in [
        '\u{3400}',
        '\u{4dbf}',
        '\u{4e00}',
        '\u{9fff}',
        '\u{20000}',
        '\u{2a6df}',
        '\u{2a700}',
        '\u{2b739}',
        '\u{2b740}',
        '\u{2b81d}',
        '\u{2b820}',
        '\u{2cea1}',
        '\u{2ceb0}',
        '\u{2ebe0}',
        '\u{30000}',
        '\u{3134a}',
        '\u{31350}',
        '\u{323af}',
    ] {
        let alphabet = [scalar.to_string()];
        assert!(
            contains(&alphabet_words(Some(&alphabet), None, "en"), "\u{20064}"),
            "{scalar:?}"
        );
    }
    // Unassigned neighbors, compatibility names (including all 12 exceptions
    // to substituting Unified_Ideograph), and a non-prefix name are excluded.
    for scalar in [
        '\u{33ff}',
        '\u{4dc0}',
        '\u{4dff}',
        '\u{a000}',
        '\u{1ffff}',
        '\u{2a6e0}',
        '\u{2a6ff}',
        '\u{2b73a}',
        '\u{2b73f}',
        '\u{2b81e}',
        '\u{2b81f}',
        '\u{2cea2}',
        '\u{2ceaf}',
        '\u{2ebe1}',
        '\u{2ebf0}',
        '\u{2ffff}',
        '\u{3134b}',
        '\u{3134f}',
        '\u{323b0}',
        '\u{f900}',
        '\u{fa0e}',
        '\u{fa0f}',
        '\u{fa11}',
        '\u{fa13}',
        '\u{fa14}',
        '\u{fa1f}',
        '\u{fa21}',
        '\u{fa23}',
        '\u{fa24}',
        '\u{fa27}',
        '\u{fa28}',
        '\u{fa29}',
        '\u{2f800}',
        '\u{3007}',
        '\u{1f210}',
    ] {
        let alphabet = [scalar.to_string()];
        assert!(
            !contains(&alphabet_words(Some(&alphabet), None, "en"), "\u{20064}"),
            "{scalar:?}"
        );
    }
    let standard = strings(&["中a", "\u{3400}\u{3401}"]);
    assert!(!contains(
        &alphabet_words(Some(&standard), None, "en"),
        "\u{20064}"
    ));
}

#[test]
fn block_names_ranges_and_filler_remain_literal() {
    for unknown in ["", "Latin", "latin ", "unknown"] {
        assert!(block_words(unknown).is_err());
    }
    let latin = block_words("latin").unwrap();
    assert!(contains(&latin, "Aß"));
    assert!(contains(&latin, "ßA"));
    assert!(!contains(&latin, "ßa"));
    assert!(!contains(&latin, "×"));
    let hangul = block_words("hangul").unwrap();
    assert!(contains(&hangul, "\u{b530}"));
    assert!(!contains(&hangul, "\u{b531}"));
    let han = block_words("han").unwrap();
    assert!(contains(&han, "\u{5058}"));
    assert!(!contains(&han, "\u{5059}"));
}
