//! Color controls at the executable interface, with independently owned streams.
#[path = "support/terminal.rs"]
mod terminal;
const PROGRAM: terminal::Program = terminal::Program;

#[test]
fn help_styles_have_exact_plain_counterpart_and_small_roles() {
    let plain = PROGRAM.invoke(&["--color=never", "--help"], b"", &[], false, false);
    let colored = PROGRAM.invoke(&["--color=always", "--help"], b"", &[], false, false);
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(plain.stdout, include_bytes!("../src/help.txt"));
    assert!(colored.status.success(), "{colored:?}");
    assert_eq!(terminal::strip_styles(&colored.stdout), plain.stdout);
    for styled in [
        b"\x1b[1;38;2;189;147;249mOperations:\x1b[0m".as_slice(),
        b"\x1b[38;2;92;184;255msum\x1b[0m",
        b"\x1b[38;2;92;184;255mq1\x1b[0m",
        b"\x1b[38;2;92;184;255mq3\x1b[0m",
        b"\x1b[38;2;92;184;255m--group\x1b[0m=FIELDS",
        b"\x1b[38;2;92;184;255mfastmash\x1b[0m",
    ] {
        assert!(
            colored
                .stdout
                .windows(styled.len())
                .any(|window| window == styled),
            "missing {styled:?}"
        );
    }
    assert!(colored.stdout.ends_with(b"\x1b[0m"));
    assert!(colored.stderr.is_empty());
    let start = colored
        .stdout
        .windows(b"Examples:".len())
        .position(|window| window == b"Examples:")
        .unwrap();
    let example = &colored.stdout[start..];
    let newline = example.iter().position(|byte| *byte == b'\n').unwrap();
    let end = example
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap();
    assert!(
        !example[newline..end].contains(&0x1b),
        "example values should retain the default foreground"
    );
}

#[test]
fn primary_help_declarations_style_names_without_their_descriptions() {
    let output = PROGRAM.invoke(&["--color=always", "--help"], b"", &[], false, false);
    assert!(output.status.success());
    let declarations: &[(&[u8], &[&str])] = &[
        (
            b"       fastmash [OPTIONS] compare",
            &["fastmash", "compare"],
        ),
        (b"       fastmash [OPTIONS] health", &["fastmash", "health"]),
        (b"  perc[:N]", &["perc"]),
        (b"  trimmean[:P]", &["trimmean"]),
        (b"Top-N selection: top:N FIELD", &["top"]),
        (b"bottom:N FIELD keeps", &["bottom"]),
        (b"Dataset comparison: compare BEFORE AFTER", &["compare"]),
        (b"Before operations, rank RESULT", &["rank"]),
        (b"Optional limit N", &["limit"]),
        (
            b"Grouping also accepts groupby, grouping or gb",
            &["groupby", "grouping", "gb"],
        ),
        (b"crosstab/ct KEY1,KEY2", &["crosstab", "ct"]),
        (b"cut/echo FIELD prints", &["cut", "echo"]),
        (b"Field modes: reverse reverses", &["reverse"]),
        (b"nop/noop consumes", &["nop", "noop", "--full"]),
        (
            b"Per-row numeric operations: getnum[:TYPE] FIELD",
            &["getnum"],
        ),
        (
            b"round, floor, ceil, trunc and frac FIELD",
            &["round", "floor", "ceil", "trunc", "frac"],
        ),
        (b"Binning: bin[:WIDTH] FIELD", &["bin"]),
        (b"strbin[:COUNT] FIELD", &["strbin"]),
        (b"Table modes: check [N lines]", &["check"]),
        (b"transpose exchanges rows", &["transpose", "--no-strict"]),
        (b"rmdup/dedup FIELD", &["rmdup", "dedup", "-s"]),
        (b"Table health: health inspects", &["health"]),
        (b"type FIELD integer|number|text", &["type"]),
        (
            b"required FIELD rejects absent and empty values",
            &["required"],
        ),
        (b"nonmissing FIELD also rejects", &["nonmissing"]),
        (b"width N declares", &["width"]),
        (b"validate requires", &["validate"]),
        (b"examples N keeps", &["examples"]),
        (b"tsv selects", &["tsv"]),
        (b"base64 FIELD encodes", &["base64"]),
        (b"debase64 FIELD strictly decodes", &["debase64"]),
        (
            b"md5, sha1, sha224, sha256, sha384 and sha512 FIELD",
            &["md5", "sha1", "sha224", "sha256", "sha384", "sha512"],
        ),
        (
            b"dirname, basename, extname and barename FIELD",
            &["dirname", "basename", "extname", "barename"],
        ),
    ];
    for &(prefix, names) in declarations {
        let line = output
            .stdout
            .split_inclusive(|byte| *byte == b'\n')
            .find(|line| terminal::strip_styles(line).starts_with(prefix))
            .unwrap();
        let styled: Vec<_> = line
            .split(|byte| *byte == 0x1b)
            .filter_map(|part| part.strip_prefix(b"[38;2;92;184;255m"))
            .collect();
        let expected: Vec<_> = names.iter().map(|name| name.as_bytes()).collect();
        assert_eq!(styled, expected, "wrong syntax roles in {prefix:?}");
    }
    let exact = b"\x1b[38;2;92;184;255mcut\x1b[0m/\x1b[38;2;92;184;255mecho\x1b[0m FIELD prints the selected fields of each record.\n";
    assert!(
        output
            .stdout
            .windows(exact.len())
            .any(|bytes| bytes == exact)
    );
    for default_text in [
        b"absolute|percent".as_slice(),
        b"[N lines] [N fields]",
        b"integer|number|text",
    ] {
        assert!(
            output
                .stdout
                .windows(default_text.len())
                .any(|bytes| bytes == default_text),
            "missing unstyled {default_text:?}"
        );
    }
    for word in ["and", "or", "lines", "fields", "absolute", "percent"] {
        let styled = format!("\x1b[38;2;92;184;255m{word}\x1b[0m");
        assert!(
            !output
                .stdout
                .windows(styled.len())
                .any(|bytes| bytes == styled.as_bytes()),
            "styled prose or value {word:?}"
        );
    }
    assert!(
        output
            .stdout
            .windows(b"\x1b[38;2;92;184;255m--\x1b[0m".len())
            .any(|bytes| bytes == b"\x1b[38;2;92;184;255m--\x1b[0m")
    );
}

#[test]
fn automatic_help_uses_its_destination_and_environment() {
    for stdout_terminal in [false, true] {
        for stderr_terminal in [false, true] {
            for (environment, eligible) in [
                (vec![], true),
                (vec![("NO_COLOR", "")], true),
                (vec![("NO_COLOR", "0")], false),
                (vec![("NO_COLOR", "false")], false),
                (vec![("TERM", "dumb")], false),
            ] {
                let output = PROGRAM.invoke(
                    &["--help"],
                    b"",
                    &environment,
                    stdout_terminal,
                    stderr_terminal,
                );
                assert!(output.status.success());
                assert_eq!(
                    output.stdout.contains(&0x1b),
                    stdout_terminal && eligible,
                    "stdout={stdout_terminal}, stderr={stderr_terminal}, {environment:?}"
                );
                assert_eq!(
                    terminal::strip_styles(&output.stdout),
                    include_bytes!("../src/help.txt")
                );
            }
        }
    }
    for mode in ["--color=always", "--color=never", "--no-color"] {
        let output = PROGRAM.invoke(
            &[mode, "--help"],
            b"",
            &[("TERM", "dumb"), ("NO_COLOR", "1")],
            true,
            false,
        );
        assert_eq!(output.stdout.contains(&0x1b), mode == "--color=always");
    }
}

#[test]
fn automatic_color_preserves_byte_environment_values_and_long_preferences() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let long_preference = vec![b'x'; 8192];
    for (name, value, suppressed) in [
        ("NO_COLOR", b"\xff".as_slice(), true),
        ("NO_COLOR", long_preference.as_slice(), true),
        ("TERM", b"dumb\xff".as_slice(), false),
    ] {
        let mut command = PROGRAM.command(&["--help"], &[]);
        command.env(name, OsStr::from_bytes(value));
        let output = terminal::output(&mut command, b"", true, false);
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(output.stdout.contains(&0x1b), !suppressed, "{name}");
        assert_eq!(
            terminal::strip_styles(&output.stdout),
            include_bytes!("../src/help.txt")
        );
    }
}

#[test]
fn color_scanning_keeps_existing_stop_points_and_exact_spelling() {
    for (args, colored) in [
        (vec!["--color", "always", "--help"], true),
        (vec!["--color=always", "--no-color", "--help"], false),
        (vec!["--no-color", "--color=always", "--help"], true),
        (vec!["--color=always", "--color=auto", "--help"], false),
        (vec!["count", "1", "--color=always", "--help"], true),
        (vec!["--color=always", "--help", "--no-color"], true),
        (vec!["--help", "--color=always"], false),
        (vec!["--color=never", "-hV", "--color=always"], false),
    ] {
        let output = PROGRAM.invoke(&args, b"", &[], false, false);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout.contains(&0x1b), colored, "{args:?}");
    }
    for args in [
        vec!["--colo=always"],
        vec!["--no-col"],
        vec!["--color"],
        vec!["--color="],
        vec!["--color=Always"],
        vec!["--no-color=yes"],
        vec!["-t", "--color=always"],
        vec!["--", "--color=always"],
    ] {
        let output = PROGRAM.invoke(&args, b"", &[], false, false);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    let prefix = PROGRAM.invoke(&["--col", "count", "1"], b"a\nb\n", &[], false, false);
    assert!(
        String::from_utf8_lossy(&prefix.stderr)
            .contains("option '--collapse-delimiter' requires an argument")
            || String::from_utf8_lossy(&prefix.stderr)
                .contains("the delimiter must be a single character"),
        "{prefix:?}"
    );
    for option in ["--n", "--hea", "--s"] {
        let output = PROGRAM.invoke(&[option], b"", &[], false, false);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("is ambiguous"),
            "{output:?}"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("'--no-color'"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("'--color'"));
    }
    let posix = PROGRAM.invoke(
        &["count", "1", "--color=always", "--help"],
        b"a\n",
        &[("POSIXLY_CORRECT", "")],
        false,
        false,
    );
    assert!(!posix.status.success());
    assert!(posix.stdout.is_empty());
}

#[test]
fn forced_color_does_not_change_data_or_version_bytes() {
    let fixtures: &[(&[&str], &[u8])] = &[
        (&["sum", "1"], b"1\n2\n"),
        (&["-H", "sum", "1"], b"value\n1\n2\n"),
        (&["--full", "nop"], b"a\tb\n\xff\t\x1b[31m\n"),
        (&["--csv", "cut", "1"], b"\"a,b\"\n\"quoted\"\n"),
        (&["health", "tsv"], b"1\tx\n2\ty\n"),
        (&["--version"], b""),
    ];
    for &(args, input) in fixtures {
        let plain = PROGRAM.invoke(args, input, &[], false, false);
        let mut colored_args = vec!["--color=always"];
        colored_args.extend(args);
        let colored = PROGRAM.invoke(&colored_args, input, &[], false, false);
        assert_eq!(colored.status, plain.status, "{args:?}");
        assert_eq!(colored.stdout, plain.stdout, "{args:?}");
        assert!(plain.status.success(), "{args:?}: {plain:?}");
    }
    // Both datasets can be the same ordinary file; color only affects eligible
    // presentation, and comparison's report is always data.
    let path =
        std::env::temp_dir().join(format!("fastmash-color-compare-{}.tsv", std::process::id()));
    std::fs::write(&path, b"1\n2\n").unwrap();
    let path = path.to_str().unwrap();
    let args = ["compare", path, path, "sum", "1"];
    let plain = PROGRAM.invoke(&args, b"", &[], false, false);
    let colored = PROGRAM.invoke(
        &["--color=always", "compare", path, path, "sum", "1"],
        b"",
        &[],
        true,
        false,
    );
    assert!(colored.status.success(), "{colored:?}");
    assert_eq!(colored.stdout, plain.stdout);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn terminal_results_style_calculations_and_selection_but_leave_pipes_plain() {
    for (args, input, heading) in [
        (
            vec!["sum", "1", "mean", "1"],
            b"1\n2\n3\n".as_slice(),
            "Summary",
        ),
        (
            vec!["-s", "-H", "-g", "team", "sum", "points", "mean", "points"],
            b"team\tpoints\nBlue\t12\nRed\t8\nBlue\t18\n",
            "Grouped summary",
        ),
        (
            vec!["-H", "top:2", "points"],
            b"team\tpoints\nBlue\t12\nRed\t8\nBlue\t18\n",
            "Top-N selection",
        ),
        (vec!["round", "1"], b"12.34\n8.76\n", "Per-row results"),
    ] {
        let styled = PROGRAM.invoke(&args, input, &[], true, false);
        assert!(styled.status.success(), "{styled:?}");
        let text = String::from_utf8(terminal::strip_styles(&styled.stdout)).unwrap();
        assert!(text.starts_with(heading), "{text}");
        assert!(text.ends_with("✔ Completed\n"), "{text}");
        assert!(String::from_utf8_lossy(&styled.stdout).contains("38;2;92;184;255"));
        let plain = PROGRAM.invoke(&args, input, &[], false, false);
        let mut forced = vec!["--color=always"];
        forced.extend(&args);
        let redirected = PROGRAM.invoke(&forced, input, &[], false, true);
        assert_eq!(redirected.stdout, plain.stdout);
        let mut disabled = vec!["--no-color"];
        disabled.extend(&args);
        assert_eq!(
            PROGRAM.invoke(&disabled, input, &[], true, false).stdout,
            plain.stdout
        );
    }
}

#[test]
fn terminal_results_keep_partial_values_without_a_success_marker() {
    let output = PROGRAM.invoke(&["round", "1"], b"12.34\noops\n", &[], true, true);
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(terminal::strip_styles(&output.stdout)).unwrap();
    assert!(text.contains("12"), "{text}");
    assert!(!text.contains("Completed"), "{text}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid numeric value in line 2"));
}

#[test]
fn terminal_results_align_unicode_labels_and_values_by_displayed_columns() {
    for (label, row) in [
        ("é", "       1       2\n"),
        ("界", "        1       2\n"),
        ("e\u{301}", "       1       2\n"),
    ] {
        let input = format!("{label}\tx\n1\t2\n");
        let output = PROGRAM.invoke(
            &["-H", "sum", "1", "sum", "2"],
            input.as_bytes(),
            &[],
            true,
            false,
        );
        assert!(output.status.success());
        let text = String::from_utf8(terminal::strip_styles(&output.stdout)).unwrap();
        assert!(text.contains(row), "{text}");
    }
    let output = PROGRAM.invoke(
        &["-H", "top:2", "points"],
        "name\tpoints\né\t2\n界\t1\n".as_bytes(),
        &[],
        true,
        false,
    );
    let text = String::from_utf8(terminal::strip_styles(&output.stdout)).unwrap();
    assert!(text.contains("     é       2\n    界       1\n"), "{text}");
}

#[test]
fn automatic_terminal_headers_do_not_read_input_labels() {
    for (args, input, label, result) in [
        (
            vec!["--header-in", "sum", "2"],
            b"label\n1\t2\n".as_slice(),
            "sum(field-2)",
            "2",
        ),
        (
            vec!["--header-in", "-g2", "sum", "1"],
            b"label\n1\tk\n2\tk\n",
            "field-2",
            "3",
        ),
        (
            vec!["--header-in", "sum", "1"],
            b"\x1b]0;spoof\x07\n1\n",
            "sum(field-1)",
            "1",
        ),
        (
            vec!["--header-in", "top:1", "1"],
            b"\x1b]0;spoof\x07\n1\n",
            "field-1",
            "1",
        ),
        (
            vec!["--csv-in", "--header-in", "sum", "1"],
            b"\"spoof\nlabel\"\n1\n",
            "sum(field-1)",
            "1",
        ),
    ] {
        let output = PROGRAM.invoke(&args, input, &[], true, false);
        assert!(output.status.success(), "{output:?}");
        let plain = terminal::strip_styles(&output.stdout);
        let text = String::from_utf8(plain).unwrap();
        assert!(text.contains(label), "{text}");
        assert!(!text.contains("spoof"), "{text}");
        let row = text.lines().rev().nth(1).unwrap();
        assert_eq!(row.split_whitespace().last(), Some(result), "{text}");
        let redirected = PROGRAM.invoke(&args, input, &[], false, false);
        assert!(redirected.status.success(), "{redirected:?}");
        assert_eq!(
            String::from_utf8_lossy(&redirected.stdout)
                .split_whitespace()
                .last(),
            Some(result)
        );
    }
}

#[test]
fn long_terminal_labels_do_not_multiply_row_padding() {
    let label = "x".repeat(64 * 1024);
    let input = format!("{label}\n1\n2\n");
    let output = PROGRAM.invoke(&["-H", "cut", "1"], input.as_bytes(), &[], true, false);
    assert!(output.status.success(), "{output:?}");
    let plain = terminal::strip_styles(&output.stdout);
    assert!(
        plain
            .windows(label.len())
            .any(|bytes| bytes == label.as_bytes())
    );
    assert!(plain.len() < label.len() + 1024, "{} bytes", plain.len());
    let text = String::from_utf8(plain).unwrap();
    assert!(text.lines().any(|row| row.trim() == "1"));
    assert!(text.lines().any(|row| row.trim() == "2"));
}
