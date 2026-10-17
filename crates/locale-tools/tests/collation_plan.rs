#![forbid(unsafe_code)]

use fastmash_locale_tools::collation_plan::{
    OrderDifference, compare_order_outputs, digit_words, punctuation_words,
};

// Handwritten in lexical order, independently of the constructor loops.
const DIGITS: [&str; 133] = [
    "0", "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0a", "1", "10", "11", "12",
    "13", "14", "15", "16", "17", "18", "19", "1a", "2", "20", "21", "22", "23", "24", "25", "26",
    "27", "28", "29", "2a", "3", "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "3a",
    "4", "40", "41", "42", "43", "44", "45", "46", "47", "48", "49", "4a", "5", "50", "51", "52",
    "53", "54", "55", "56", "57", "58", "59", "5a", "6", "60", "61", "62", "63", "64", "65", "66",
    "67", "68", "69", "6a", "7", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "7a",
    "8", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "8a", "9", "90", "91", "92",
    "93", "94", "95", "96", "97", "98", "99", "9a", "a", "a0", "a1", "a2", "a3", "a4", "a5", "a6",
    "a7", "a8", "a9", "b", "z",
];

fn output(words: &[&str]) -> Vec<u8> {
    words
        .iter()
        .flat_map(|word| word.bytes().chain(b"\t1\n".iter().copied()))
        .collect()
}

fn difference(candidate: &[&str], reference: &[&str]) -> OrderDifference {
    compare_order_outputs(&output(candidate), &output(reference)).unwrap()
}

#[test]
fn digit_plan_matches_all_133_handwritten_words() {
    let words = digit_words();
    let actual: Vec<_> = words.iter().map(String::as_str).collect();
    assert_eq!(actual.as_slice(), DIGITS.as_slice());
}

#[test]
fn punctuation_plan_has_retained_count_sorted_uniqueness_and_literal_boundaries() {
    let words = punctuation_words();
    // Retained historical report count. This alone is not full plan parity.
    assert_eq!(words.len(), 1_983);
    assert!(words.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(words.first().map(String::as_str), Some(" a"));
    assert_eq!(words.last().map(String::as_str), Some("（中"));
    assert!(words.iter().all(|word| {
        word.bytes()
            .all(|byte| !matches!(byte, b'\r' | b'\n' | b'\t'))
    }));
}

#[test]
fn punctuation_plan_preserves_literal_contexts_spaces_and_symbol_exclusions() {
    let words = punctuation_words();
    for expected in [
        "!",
        "!a",
        "a!",
        "a!b",
        "!1",
        "1!2",
        "A!",
        "!!",
        "a!!b",
        "¼",
        "¼a",
        "a¼",
        "a¼b",
        "¼1",
        "1¼2",
        "A¼",
        "¼¼",
        "a¼¼b",
        "−",
        "−a",
        "a−",
        "a−b",
        "−1",
        "1−2",
        "A−",
        "−−",
        "a−−b",
        "!~",
        "~!",
        "\"\\",
        "\\\"",
        "<>",
        "><",
        "__",
        "||",
        " a",
        "a ",
        "a b",
        "a  b",
        "a b c",
        "a\u{a0}-b",
        "a-\u{a0}b",
        "Cia\u{a0}Ltda",
        "Cia.\u{a0}Ltda",
        "ul\u{a0}.Dluga",
        "ul.\u{a0}Dluga",
        "-",
        "‐",
        "–",
        "—",
        "×",
        "÷",
        "€",
        "₹",
        "№",
        "™",
        "♪",
        "-1.5",
        "1e-3",
        "-.5",
        "1,5",
        "1.5",
        "c.12_13del",
        "c.12-1G>A",
        "c.12+1G>A",
        "c.121G>A",
        "p.Arg97Gly",
        "p.(Arg97Gly)",
        "p.Arg97*",
        "chr1:123-456",
        "file1.txt",
        "File1.txt",
        "file(1).txt",
        "o'neil",
        "O’Neil",
        "（中",
        "中（",
        "中国（北京）",
        "中国北京（）",
        "-中a_",
        "č–a",
    ] {
        assert!(
            words.iter().any(|word| word == expected),
            "missing {expected:?}"
        );
    }
    for excluded in [
        "ª", "\u{ad}", "²", "³", "¹", "º", "aªb", "1²2", " ", "0", "a", "\n", "\t", "Cia Ltda",
        "a- b",
    ] {
        assert!(
            !words.iter().any(|word| word == excluded),
            "unexpected {excluded:?}"
        );
    }
    assert_eq!(
        words
            .iter()
            .filter(|word| word.as_str() == "c.12+1G>A")
            .count(),
        1
    );
    assert_eq!(words.iter().filter(|word| word.as_str() == "!!").count(), 1);
}

#[test]
fn order_comparison_keeps_literal_ties_duplicate_ranks_and_reference_only_behavior() {
    type OrderCase<'a> = (&'a [&'a str], &'a [&'a str], bool, &'a [&'a str]);
    let cases: &[OrderCase<'_>] = &[
        (&[], &[], true, &[]),
        (&["a", "b", "c"], &["a", "b", "c"], true, &[]),
        (&["b", "a", "c"], &["a", "b", "c"], false, &["b"]),
        (&["a", "c", "b", "d"], &["a", "b", "c", "d"], false, &["c"]),
        (
            &["b", "a", "d", "c"],
            &["a", "b", "c", "d"],
            false,
            &["b", "d"],
        ),
        (
            &["d", "c", "b", "a"],
            &["a", "b", "c", "d"],
            false,
            &["b", "c", "d"],
        ),
        (&["b"], &["a", "b"], false, &[]),
        (&[], &["a"], false, &[]),
        (&["a", "a"], &["a"], false, &[]),
        (&["b", "b", "a"], &["a", "b"], false, &["b"]),
        // The final reference occurrence owns its rank.
        (&["a", "b"], &["a", "b", "a"], false, &["a"]),
        // Original predecessors are keyed by word, including later overwrites.
        (&["b", "c", "a", "b"], &["a", "b", "c"], false, &["c"]),
        (
            &["b", "c", "d", "a", "b"],
            &["a", "b", "c", "d"],
            false,
            &[],
        ),
    ];
    for &(candidate, reference, equal, displaced) in cases {
        let actual = difference(candidate, reference);
        assert_eq!(actual.equal, equal, "{candidate:?} versus {reference:?}");
        let moved: Vec<_> = actual.displaced.iter().map(String::as_str).collect();
        assert_eq!(moved, displaced, "{candidate:?} versus {reference:?}");
    }
    let error = compare_order_outputs(b"x\t1\n", b"a\t1\n").unwrap_err();
    assert!(error.to_string().contains("reference rank"));
}

#[test]
fn output_projection_preserves_byte_lines_first_fields_and_strict_utf8() {
    for (candidate, reference) in [
        (
            b"a\t1\r\nb\t2\rc\t3\n".as_slice(),
            b"a\t9\nb\t8\nc\t7\n".as_slice(),
        ),
        (b"a\t\xff\r\nb\t2\t\xfe\n".as_slice(), b"a\nb\t0".as_slice()),
        (b"\t1\n".as_slice(), b"\r\n".as_slice()),
        (b"\r\r\n".as_slice(), b"\n\n".as_slice()),
        (b"a\n".as_slice(), b"a".as_slice()),
        (b"".as_slice(), b"".as_slice()),
        (
            b" a\t1\na \t2\na  b\t3\n".as_slice(),
            b" a\na \na  b\n".as_slice(),
        ),
        (b"a\0b\t1\n".as_slice(), b"a\0b\n".as_slice()),
    ] {
        let actual = compare_order_outputs(candidate, reference).unwrap();
        assert!(actual.equal, "{candidate:?} versus {reference:?}");
        assert!(actual.displaced.is_empty());
    }
    assert!(!compare_order_outputs(b"", b"\n").unwrap().equal);
    for candidate in [
        b"a\x0bb\t1\n".as_slice(),
        b"a\x0cb\t1\n".as_slice(),
        b"a\xc2\x85b\t1\n".as_slice(),
        b" a\t1\n".as_slice(),
        b"a \t1\n".as_slice(),
    ] {
        assert!(compare_order_outputs(candidate, b"a\t1\nb\t1\n").is_err());
    }
    for (candidate, reference) in [
        (b"\xff\t1\n".as_slice(), b"a\n".as_slice()),
        (b"a\n".as_slice(), b"\xff\t1\n".as_slice()),
    ] {
        assert!(compare_order_outputs(candidate, reference).is_err());
    }
    let actual = compare_order_outputs(
        "é\t2\ne\u{301}\t1\n".as_bytes(),
        "e\u{301}\t9\né\t8\n".as_bytes(),
    )
    .unwrap();
    assert!(!actual.equal);
    assert_eq!(actual.displaced, ["é"]);
}

#[test]
fn displacement_result_is_complete_sorted_and_not_capped_at_forty() {
    let reference = &DIGITS[..45];
    let candidate: Vec<_> = reference.iter().rev().copied().collect();
    let actual = difference(&candidate, reference);
    assert!(!actual.equal);
    let moved: Vec<_> = actual.displaced.iter().map(String::as_str).collect();
    assert_eq!(moved.as_slice(), &DIGITS[1..45]);
    assert_eq!(moved.len(), 44);
}

#[test]
fn unique_four_word_orders_match_an_independent_quadratic_lcs_length() {
    // All 24 orders against one fixed reference cover relative-rank permutations.
    let permutations = [
        ["a", "b", "c", "d"],
        ["a", "b", "d", "c"],
        ["a", "c", "b", "d"],
        ["a", "c", "d", "b"],
        ["a", "d", "b", "c"],
        ["a", "d", "c", "b"],
        ["b", "a", "c", "d"],
        ["b", "a", "d", "c"],
        ["b", "c", "a", "d"],
        ["b", "c", "d", "a"],
        ["b", "d", "a", "c"],
        ["b", "d", "c", "a"],
        ["c", "a", "b", "d"],
        ["c", "a", "d", "b"],
        ["c", "b", "a", "d"],
        ["c", "b", "d", "a"],
        ["c", "d", "a", "b"],
        ["c", "d", "b", "a"],
        ["d", "a", "b", "c"],
        ["d", "a", "c", "b"],
        ["d", "b", "a", "c"],
        ["d", "b", "c", "a"],
        ["d", "c", "a", "b"],
        ["d", "c", "b", "a"],
    ];
    let reference = ["a", "b", "c", "d"];
    for candidate in permutations {
        let mut lengths = [[0_usize; 5]; 5];
        for (left, word) in candidate.iter().enumerate() {
            for (right, other) in reference.iter().enumerate() {
                lengths[left + 1][right + 1] = if word == other {
                    lengths[left][right] + 1
                } else {
                    lengths[left][right + 1].max(lengths[left + 1][right])
                };
            }
        }
        let actual = difference(&candidate, &reference);
        assert_eq!(actual.displaced.len(), 4 - lengths[4][4], "{candidate:?}");
    }
}
