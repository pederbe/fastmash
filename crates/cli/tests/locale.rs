//! Locale resolution observed through the command: which environments are
//! accepted, which are refused, and what the refusal says.
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};

fn run(variables: &[(&str, &str)], args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastmash"))
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .envs(variables.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A refused command can exit before reading its input (broken pipe).
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

fn assert_output(out: &Output, status: i32, stdout: &[u8]) {
    assert_eq!(out.status.code(), Some(status), "{out:?}");
    assert_eq!(out.stdout, stdout, "{out:?}");
}

#[test]
fn english_locales_with_en_us_number_rules_are_accepted() {
    // Each alias copies en_US's decimal point, thousands separator and grouping
    // in glibc's localedata (checked against glibc 2.43).
    for lang in [
        "en_GB.UTF-8",
        "en_IE.UTF-8",
        "en_AU.UTF-8",
        "en_NZ.UTF-8",
        "en_CA.UTF-8",
        "en_ZA.utf8",
    ] {
        let out = run(&[("LANG", lang)], &["sum", "1"], b"1.5\n3\n");
        assert_output(&out, 0, b"4.5\n");
        let out = run(
            &[("LANG", lang)],
            &["--format", "%'.1f", "sum", "1"],
            b"1234567\n",
        );
        assert_output(&out, 0, b"1,234,567.0\n");
    }
}

#[test]
fn german_locales_with_de_de_number_rules_are_accepted() {
    for lang in ["de_AT.UTF-8", "de_LU.UTF-8", "de_BE.utf8"] {
        let out = run(&[("LANG", lang)], &["sum", "1"], b"1,5\n3\n");
        assert_output(&out, 0, b"4,5\n");
    }
}

#[test]
fn every_locale_uses_its_own_glibc_number_rules() {
    // Expected output is GNU datamash's under glibc 2.43's compiled locales.
    let out = run(&[("LANG", "en_DK.UTF-8")], &["sum", "1"], b"1,5\n3\n");
    assert_output(&out, 0, b"4,5\n");
    for (lang, input, grouped) in [
        // U+202F narrow no-break space thousands separator, decimal comma.
        (
            "nb_NO.UTF-8",
            b"1,5\n1234567\n",
            "1\u{202f}234\u{202f}568,50\n",
        ),
        // U+2019 right single quotation mark, decimal point.
        (
            "de_CH.UTF-8",
            b"1.5\n1234567\n",
            "1\u{2019}234\u{2019}568.50\n",
        ),
        // Groups of 3, then of 2.
        ("as_IN.UTF-8", b"1.5\n1234567\n", "12,34,568.50\n"),
    ] {
        let out = run(&[("LANG", lang)], &["--format", "%'.2f", "sum", "1"], input);
        assert_output(&out, 0, grouped.as_bytes());
    }
}

#[test]
fn locales_without_a_one_byte_decimal_point_or_utf8_are_refused_for_numbers() {
    // ps_AF's decimal separator is U+066B; fr_FR without a codeset is Latin-1.
    for lang in ["ps_AF.UTF-8", "fr_FR", "nb_NO.UTF-8@euro"] {
        let out = run(&[("LANG", lang)], &["sum", "1"], b"1\n");
        assert_output(&out, 77, b"");
    }
}

#[test]
fn commands_that_neither_read_nor_print_numbers_accept_any_locale() {
    let env = [("LANG", "ps_AF.UTF-8")];
    let out = run(&env, &["transpose"], b"a\tb\n1\t2\n");
    assert_output(&out, 0, b"a\t1\nb\t2\n");
    let out = run(
        &env,
        &["-g", "1", "first", "2", "collapse", "2", "count", "2"],
        b"a\tx\na\ty\n",
    );
    assert_output(&out, 0, b"a\tx\tx,y\t2\n");
    let out = run(
        &env,
        &["cut", "2", "md5", "1", "basename", "1"],
        b"/a/b\tz\n",
    );
    assert_output(&out, 0, b"z\taee31d71a4dce5fe24481547cc863476\tb\n");
}

#[test]
fn counts_need_a_supported_numeric_locale_only_with_an_explicit_format() {
    let env = [("LANG", "ps_AF.UTF-8")];
    let out = run(&env, &["--format", "%.2f", "count", "1"], b"a\n");
    assert_output(&out, 77, b"");
    let out = run(&env, &["--round", "1", "countunique", "1"], b"a\n");
    assert_output(&out, 77, b"");
}

#[test]
fn numeric_refusal_names_the_setting_and_how_to_change_it() {
    let out = run(&[("LANG", "ps_AF.UTF-8")], &["sum", "1"], b"1\n");
    assert_output(&out, 77, b"");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fastmash: unsupported numeric locale ‘ps_AF.UTF-8’ from LANG\n\
         hint: set LC_NUMERIC=C.UTF-8, or a UTF-8 locale whose decimal separator is one byte\n"
    );
    let out = run(&[("LC_ALL", "fr_FR")], &["sum", "1"], b"1\n");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fastmash: unsupported numeric locale 'fr_FR' from LC_ALL\n\
         hint: set LC_ALL=C.UTF-8, or a UTF-8 locale whose decimal separator is one byte\n"
    );
}

#[test]
fn sorting_refusal_names_the_setting_and_how_to_change_it() {
    // glibc's Czech collation sorts digits after letters; Unicode's does not.
    let out = run(
        &[("LANG", "cs_CZ.UTF-8")],
        &["-s", "-g", "1", "first", "2"],
        b"b\t1\na\t2\n",
    );
    assert_output(&out, 77, b"");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fastmash: unsupported sorting locale ‘cs_CZ.UTF-8’ from LANG\n\
         hint: set LC_COLLATE=C.UTF-8 for byte order; fastmash --help lists the language locales\n"
    );
}

#[test]
fn command_errors_under_an_unknown_numeric_locale_stay_ordinary_errors() {
    let env = [("LANG", "ps_AF.UTF-8")];
    let typo = run(&env, &["-g", "1", "firts", "2"], b"a\t1\n");
    assert_output(&typo, 1, b"");
    assert!(
        String::from_utf8_lossy(&typo.stderr).contains("firts"),
        "{typo:?}"
    );
    // A comma in a parameter separates fields under every locale, as in GNU,
    // so it stays an ordinary error; a valid numeric command is refused.
    let comma = run(&env, &["trimmean:0,2", "1"], b"1\n");
    assert_output(&comma, 1, b"");
    let numeric = run(&env, &["trimmean:0.2", "1"], b"1\n");
    assert_output(&numeric, 77, b"");
    assert!(
        String::from_utf8_lossy(&numeric.stderr).contains("numeric locale"),
        "{numeric:?}"
    );
}

#[test]
fn language_locales_sort_like_glibc() {
    // Expected output is GNU datamash with GNU sort under glibc 2.43.
    let sorted =
        |lang, input: &[u8]| run(&[("LANG", lang)], &["-s", "-g", "1", "count", "1"], input);
    // mt_MT sorts uppercase first, unlike en_US.
    assert_output(
        &sorted("mt_MT.UTF-8", b"a\nA\nb\n"),
        0,
        b"A\t1\na\t1\nb\t1\n",
    );
    // Slovenian's auxiliary letters \u{107} and \u{111} sort differently in
    // glibc, so its sorting is refused; so is en_CA's (\u{c6} and \u{152}).
    for lang in ["sl_SI.UTF-8", "en_CA.UTF-8", "sv_SE.UTF-8"] {
        assert_output(&sorted(lang, b"b\na\n"), 77, b"");
    }
    assert_output(
        &sorted("en_US.UTF-8", b"A\na\nb\n"),
        0,
        b"a\t1\nA\t1\nb\t1\n",
    );
    // Spanish sorts ñ as a letter after n.
    assert_output(
        &sorted("es_ES.UTF-8", "o\nñ\nn\nnz\n".as_bytes()),
        0,
        "n\t1\nnz\t1\nñ\t1\no\t1\n".as_bytes(),
    );
}

#[test]
fn case_folding_is_refused_where_glibc_does_not_fold_i() {
    // glibc's Turkic character types map i and I to dotted and dotless
    // letters, so GNU's byte folding leaves them unchanged.
    let tr = [("LANG", "tr_TR.UTF-8")];
    let out = run(&tr, &["-i", "-g", "1", "count", "1"], b"i\nI\n");
    assert_output(&out, 77, b"");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fastmash: unsupported -i under character type ‘tr_TR.UTF-8’ from LANG\n\
         hint: this locale does not fold i and I; set LC_CTYPE=C.UTF-8 or omit -i\n"
    );
    let ascii = [("LANG", "tr_TR.UTF-8"), ("LC_CTYPE", "C.UTF-8")];
    let out = run(&ascii, &["-i", "-g", "1", "count", "1"], b"i\nI\n");
    assert_output(&out, 0, b"i\t2\n");
    let out = run(&tr, &["-g", "1", "count", "1"], b"i\n");
    assert_output(&out, 0, b"i\t1\n");
    let out = run(&tr, &["-i", "crosstab", "1,2"], b"I\tx\ni\tx\n");
    assert_output(&out, 77, b"");
    // GNU's rmdup compares keys exactly, so -i without sorting changes nothing.
    let out = run(&tr, &["-i", "rmdup", "2"], b"k\tI\nk\ti\n");
    assert_output(&out, 0, b"k\tI\nk\ti\n");
    let out = run(&tr, &["-s", "-i", "rmdup", "2"], b"k\tI\nk\ti\n");
    assert_output(&out, 77, b"");
    // tt_RU@iqtelif copies tr_TR's character type in glibc's localedata.
    let tt = [("LC_ALL", "tt_RU.UTF-8@iqtelif")];
    let out = run(&tt, &["-i", "-g", "1", "count", "1"], b"i\n");
    assert_output(&out, 77, b"");
}

#[test]
fn names_without_a_codeset_are_accepted_where_glibc_defaults_to_utf8() {
    // hi_IN and az_AZ are UTF-8 by default in glibc; fr_FR is Latin-1.
    let out = run(&[("LANG", "hi_IN")], &["sum", "1"], b"1.5\n2\n");
    assert_output(&out, 0, b"3.5\n");
    let out = run(&[("LANG", "az_AZ")], &["sum", "1"], b"1,5\n2\n");
    assert_output(&out, 0, b"3,5\n");
    let out = run(&[("LANG", "fr_FR")], &["sum", "1"], b"1,5\n");
    assert_output(&out, 77, b"");
}

#[test]
fn codeset_spelling_is_case_insensitive() {
    for lang in ["en_US.UTF8", "en_GB.Utf-8", "de_AT.utf-8"] {
        let out = run(
            &[("LANG", lang)],
            &["count", "1", "--format", "%.1f"],
            b"x\n",
        );
        assert_output(
            &out,
            0,
            if lang.starts_with("de") {
                b"1,0\n"
            } else {
                b"1.0\n"
            },
        );
    }
}

#[test]
fn bucket_numbers_that_may_print_a_decimal_point_need_a_numeric_locale() {
    let env = [("LANG", "ps_AF.UTF-8")];
    let small = run(&env, &["strbin:1000", "1"], b"x\n");
    assert_eq!(small.status.code(), Some(0), "{small:?}");
    let out = run(&env, &["strbin:1000000000000000", "1"], b"x\n");
    assert_output(&out, 77, b"");
}
