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
        b"\x1b[1mOperations:\x1b[0m".as_slice(),
        b"\x1b[36msum\x1b[0m",
        b"\x1b[36mq1\x1b[0m",
        b"\x1b[36mq3\x1b[0m",
        b"\x1b[36m--group\x1b[0m=FIELDS",
        b"\x1b[36mfastmash\x1b[0m",
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
            b"       fastmash [INPUT/OUTPUT OPTIONS] compare",
            &["fastmash", "compare", "rank", "limit"],
        ),
        (
            b"fastmash [INPUT OPTIONS] health",
            &["fastmash", "health", "examples", "type"],
        ),
        (
            b"         [required FIELD]",
            &["required", "nonmissing", "width", "validate", "tsv"],
        ),
        (b"  perc[:N]", &["perc"]),
        (b"  trimmean[:P]", &["trimmean"]),
        (b"Top-N selection: top:N FIELD", &["top"]),
        (b"bottom:N FIELD selects", &["bottom"]),
        (b"Dataset comparison: compare BEFORE AFTER", &["compare"]),
        (b"Before operations, rank RESULT", &["rank"]),
        (b"Optional limit N", &["limit"]),
        (
            b"Grouping also accepts groupby, grouping or gb",
            &["groupby", "grouping", "gb"],
        ),
        (b"Crosstab/ct KEY1,KEY2", &["Crosstab", "ct"]),
        (b"cut/echo FIELD is a per-row", &["cut", "echo"]),
        (
            b"Field modes: reverse reverses",
            &["reverse", "--no-strict"],
        ),
        (
            b"changing widths); nop/noop consumes",
            &["nop", "noop", "--full"],
        ),
        (
            b"Per-row numeric operations: getnum[:TYPE] FIELD",
            &["getnum"],
        ),
        (
            b"round, floor, ceil, trunc and frac FIELD",
            &["round", "floor", "ceil", "trunc", "frac"],
        ),
        (b"Binning: bin[:WIDTH] FIELD", &["bin"]),
        (b"width 100). strbin[:COUNT] FIELD", &["strbin"]),
        (b"Table modes: check [N lines]", &["check"]),
        (b"transpose exchanges rows", &["transpose", "--no-strict"]),
        (b"rmdup/dedup FIELD", &["rmdup", "dedup"]),
        (b"Table health: health inspects", &["health"]),
        (b"type FIELD integer|number|text", &["type"]),
        (
            b"required FIELD rejects absent and empty values; nonmissing FIELD",
            &["required", "nonmissing"],
        ),
        (
            b"rule. Literal NA can pass required/text checks. width N declares",
            &["width"],
        ),
        (b"findings. validate requires", &["validate"]),
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
            .filter_map(|part| part.strip_prefix(b"[36m"))
            .collect();
        let expected: Vec<_> = names.iter().map(|name| name.as_bytes()).collect();
        assert_eq!(styled, expected, "wrong syntax roles in {prefix:?}");
    }
    let exact = b"\x1b[36mcut\x1b[0m/\x1b[36mecho\x1b[0m FIELD is a per-row operation that prints the selected fields.\n";
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
        let styled = format!("\x1b[36m{word}\x1b[0m");
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
            .windows(b"\x1b[36m--\x1b[0m".len())
            .any(|bytes| bytes == b"\x1b[36m--\x1b[0m")
    );
}

#[test]
fn negative_numeric_values_keep_the_default_foreground() {
    let output = PROGRAM.invoke(&["--color=always", "--help"], b"", &[], false, false);
    assert!(output.status.success());
    for line in output.stdout.split_inclusive(|byte| *byte == b'\n') {
        let plain = terminal::strip_styles(line);
        if plain
            .windows(4)
            .any(|bytes| bytes == b"-nan" || bytes == b"-inf")
        {
            assert!(!line.contains(&0x1b), "styled numeric value in {line:?}");
        }
    }
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
        let colored = PROGRAM.invoke(&colored_args, input, &[], true, false);
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
