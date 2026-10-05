//! Byte-preserving GNU invocation scanning; operation grammar lives in grammar.rs.

#[cfg(test)]
use super::command_memory::FAIL_RESERVATION;
use super::command_memory::reserve;
use super::terminal_style::ColorMode;
use super::{Failure, failure, records::Separator, unsupported, unsupported_hint};
use std::{
    borrow::Cow,
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
};

pub(super) enum Action {
    Calculate(Box<Options>),
    Help,
    Version,
}

pub(super) struct Scan {
    pub action: Result<Action, Failure>,
    pub color: ColorMode,
}

pub(super) struct Options {
    pub group: Option<OsString>,
    pub operands: Vec<OsString>,
    pub input: Separator,
    pub output: u8,
    pub csv_out: bool,
    pub csv_in: bool,
    pub(super) supplied_output: bool,
    pub collapse: u8,
    pub record_end: u8,
    pub full: bool,
    pub linewise: bool,
    pub crosstab: bool,
    pub strict: bool,
    pub header_in: bool,
    pub header_out: bool,
    pub explicit_header_out: bool,
    pub result_names: super::result_names::ResultNames,
    pub skip_comments: bool,
    pub narm: bool,
    pub vnlog: bool,
    pub filler: Cow<'static, [u8]>,
    pub(super) explicit_output: bool,
    pub sort: bool,
    pub ignore_case: bool,
    pub seed: Option<u32>,
    pub presentation: super::presentation::Presentation,
    pub locale: super::locale::Policy,
    /// `FASTMASH_SORT_MEMORY_BYTES` from the environment, which only the
    /// native sort reads (and refuses when it is invalid).
    pub sort_memory: Option<OsString>,
    pub(super) controls: super::preparation::Controls,
}

#[derive(Clone, Copy)]
pub(super) enum Setting {
    Full,
    TableOnly,
    Filler,
    Vnlog,
    ZeroTerminated,
    Input,
    Output,
    Collapse,
    Group,
    Whitespace,
    HeaderIn,
    HeaderOut,
    Headers,
    ResultName,
    CsvOut,
    CsvIn,
    Csv,
    Comments,
    Na,
    Sort,
    IgnoreCase,
    Seed,
    Format,
    Round,
    Color,
    NoColor,
    Help,
    Version,
    Unavailable,
}

struct Descriptor {
    long: &'static [u8],
    short: Option<u8>,
    value: bool,
    setting: Setting,
}

// GNU declaration order is observable in ambiguous-option diagnostics. Features
// not yet delivered still participate in matching; private hooks remain omitted.
const OPTIONS: &[Descriptor] = &[
    descriptor(
        b"zero-terminated",
        Some(b'z'),
        false,
        Setting::ZeroTerminated,
    ),
    descriptor(b"field-separator", Some(b't'), true, Setting::Input),
    descriptor(b"whitespace", Some(b'W'), false, Setting::Whitespace),
    descriptor(b"group", Some(b'g'), true, Setting::Group),
    descriptor(b"ignore-case", Some(b'i'), false, Setting::IgnoreCase),
    descriptor(b"skip-comments", Some(b'C'), false, Setting::Comments),
    descriptor(b"header-in", None, false, Setting::HeaderIn),
    descriptor(b"header-out", None, false, Setting::HeaderOut),
    descriptor(b"headers", Some(b'H'), false, Setting::Headers),
    descriptor(b"vnlog", None, false, Setting::Vnlog),
    descriptor(b"full", Some(b'f'), false, Setting::Full),
    descriptor(b"filler", Some(b'F'), true, Setting::Filler),
    descriptor(b"format", None, true, Setting::Format),
    descriptor(b"output-delimiter", None, true, Setting::Output),
    descriptor(b"collapse-delimiter", Some(b'c'), true, Setting::Collapse),
    descriptor(b"sort", Some(b's'), false, Setting::Sort),
    // Existing required-value semantics differ from GNU 1.9's defective long
    // seed declaration: `--seed` takes a required value, as `-S` does.
    descriptor(b"seed", Some(b'S'), true, Setting::Seed),
    descriptor(b"no-strict", None, false, Setting::TableOnly),
    descriptor(b"narm", None, false, Setting::Na),
    descriptor(b"round", Some(b'R'), true, Setting::Round),
    descriptor(b"sort-cmd", None, true, Setting::Unavailable),
    descriptor(b"help", Some(b'h'), false, Setting::Help),
    descriptor(b"version", Some(b'V'), false, Setting::Version),
];

// Extensions use exact spelling and never enter GNU's prefix matching.
const RESULT_NAME: Descriptor = descriptor(b"result-name", None, true, Setting::ResultName);
const CSV_OUT: Descriptor = descriptor(b"csv-out", None, false, Setting::CsvOut);
const CSV_IN: Descriptor = descriptor(b"csv-in", None, false, Setting::CsvIn);
const CSV: Descriptor = descriptor(b"csv", None, false, Setting::Csv);
const COLOR: Descriptor = descriptor(b"color", None, true, Setting::Color);
const NO_COLOR: Descriptor = descriptor(b"no-color", None, false, Setting::NoColor);
const EXTENSIONS: &[&Descriptor] = &[&RESULT_NAME, &CSV_OUT, &CSV_IN, &CSV, &COLOR, &NO_COLOR];

const fn descriptor(
    long: &'static [u8],
    short: Option<u8>,
    value: bool,
    setting: Setting,
) -> Descriptor {
    Descriptor {
        long,
        short,
        value,
        setting,
    }
}

fn copy_argument(bytes: &[u8]) -> Result<OsString, Failure> {
    let mut copy = Vec::new();
    reserve(&mut copy, bytes.len())?;
    copy.extend_from_slice(bytes);
    Ok(OsString::from_vec(copy))
}

fn push_operand(operands: &mut Vec<OsString>, arg: &OsString) -> Result<(), Failure> {
    reserve(operands, 1)?;
    operands.push(copy_argument(arg.as_bytes())?);
    Ok(())
}

pub(super) fn collect_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Vec<OsString>, Failure> {
    let mut collected = Vec::new();
    for arg in args {
        reserve(&mut collected, 1)?;
        collected.push(arg);
    }
    Ok(collected)
}

fn diagnostic(parts: &[&[u8]], name: &[u8]) -> Failure {
    let mut message = Vec::new();
    for part in parts {
        message.extend_from_slice(part);
    }
    message.extend_from_slice(b"\nTry '");
    message.extend_from_slice(name);
    message.extend_from_slice(b" --help' for more information.\n");
    failure(message)
}

fn exact_long(token: &[u8]) -> Option<&'static Descriptor> {
    EXTENSIONS
        .iter()
        .copied()
        .chain(OPTIONS)
        .find(|option| option.long == token)
}

/// Help recognizes complete option names, never GNU's abbreviations or a
/// prefix of a numeric value such as `-inf`.
pub(super) fn documented_option(token: &[u8]) -> bool {
    if token == b"--" {
        true
    } else if let Some(long) = token.strip_prefix(b"--") {
        exact_long(long).is_some()
    } else {
        token.len() == 2
            && token[0] == b'-'
            && OPTIONS.iter().any(|option| option.short == Some(token[1]))
    }
}

fn match_long(token: &[u8], spelling: &[u8], name: &[u8]) -> Result<&'static Descriptor, Failure> {
    if let Some(exact) = exact_long(token) {
        return Ok(exact);
    }
    let mut matches = OPTIONS
        .iter()
        .filter(|option| option.long.starts_with(token));
    match (matches.next(), matches.next()) {
        (Some(option), None) => return Ok(option),
        (None, _) => {
            return Err(diagnostic(
                &[b"unrecognized option '", spelling, b"'"],
                name,
            ));
        }
        _ => {}
    }
    let mut message = Vec::new();
    message.extend_from_slice(b"option '");
    message.extend_from_slice(spelling);
    message.extend_from_slice(b"' is ambiguous; possibilities:");
    for option in OPTIONS
        .iter()
        .filter(|option| option.long.starts_with(token))
    {
        message.extend_from_slice(b" '--");
        message.extend_from_slice(option.long);
        message.push(b'\'');
    }
    Err(diagnostic(&[&message], name))
}

fn unavailable(bytes: &[u8]) -> Failure {
    let escaped: String = bytes
        .iter()
        .take(80)
        .flat_map(|&byte| std::ascii::escape_default(byte))
        .map(char::from)
        .collect();
    unsupported_hint(
        &format!("unsupported option '{escaped}'"),
        "use --help to see supported options.",
    )
}
fn seed(bytes: &[u8]) -> Result<u32, Failure> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut at = bytes
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .unwrap_or(bytes.len());
    let negative = match bytes.get(at) {
        Some(b'+') => {
            at += 1;
            false
        }
        Some(b'-') => {
            at += 1;
            true
        }
        _ => false,
    };
    let start = at;
    let mut value = 0u64;
    while let Some(digit) = bytes.get(at).and_then(|byte| byte.checked_sub(b'0')) {
        if digit > 9 {
            break;
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit)))
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| failure(b"invalid seed\n".to_vec()))?;
        at += 1;
    }
    if at == start || at != bytes.len() || (negative && value != 0) {
        return Err(failure(b"invalid seed\n".to_vec()));
    }
    Ok(value as u32)
}

#[cfg(test)]
pub(super) fn parse(
    args: &[OsString],
    name: &[u8],
    posixly_correct: bool,
) -> Result<Action, Failure> {
    parse_with_policy(args, name, posixly_correct, Default::default())
}

#[cfg(test)]
pub(super) fn parse_with_policy(
    args: &[OsString],
    name: &[u8],
    posixly_correct: bool,
    locale: super::locale::Policy,
) -> Result<Action, Failure> {
    scan_with_policy(args, name, posixly_correct, locale).action
}

pub(super) fn scan_with_policy(
    args: &[OsString],
    name: &[u8],
    posixly_correct: bool,
    locale: super::locale::Policy,
) -> Scan {
    let mut color = ColorMode::Auto;
    let action = parse_options(args, name, posixly_correct, locale, false, &mut color);
    Scan { action, color }
}

fn parse_options(
    args: &[OsString],
    name: &[u8],
    posixly_correct: bool,
    locale: super::locale::Policy,
    allow_unavailable: bool,
    color: &mut ColorMode,
) -> Result<Action, Failure> {
    if args.iter().any(|arg| arg.as_bytes().contains(&0)) {
        return Err(unsupported("NUL bytes in arguments are unsupported"));
    }
    let mut options = Options {
        group: None,
        operands: Vec::new(),
        input: Separator::Literal(b'\t'),
        output: b'\t',
        csv_out: false,
        csv_in: false,
        supplied_output: false,
        collapse: b',',
        record_end: b'\n',
        full: false,
        linewise: false,
        crosstab: false,
        strict: true,
        header_in: false,
        header_out: false,
        explicit_header_out: false,
        result_names: Default::default(),
        skip_comments: false,
        narm: false,
        vnlog: false,
        filler: Cow::Borrowed(b"N/A"),
        explicit_output: false,
        sort: false,
        ignore_case: false,
        seed: None,
        presentation: super::presentation::Presentation {
            profile: locale.numeric,
            utf8: locale.utf8,
            ..Default::default()
        },
        locale,
        sort_memory: None,
        controls: Default::default(),
    };
    let mut explicit_output = None;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        let bytes = arg.as_bytes();
        if bytes == b"--" {
            for arg in &args[at..] {
                push_operand(&mut options.operands, arg)?;
            }
            break;
        }
        if bytes.len() < 2 || bytes[0] != b'-' {
            push_operand(&mut options.operands, arg)?;
            if posixly_correct {
                for arg in &args[at..] {
                    push_operand(&mut options.operands, arg)?;
                }
                break;
            }
            continue;
        }
        let long = bytes[1] == b'-';
        let mut letter = 1;
        loop {
            let (option, attached) = if long {
                let (token, value) = match bytes[2..].iter().position(|byte| *byte == b'=') {
                    Some(equal) => (&bytes[2..2 + equal], Some(&bytes[3 + equal..])),
                    None => (&bytes[2..], None),
                };
                let option = match_long(token, bytes, name)?;
                if !option.value && value.is_some() {
                    return Err(diagnostic(
                        &[b"option '--", option.long, b"' doesn't allow an argument"],
                        name,
                    ));
                }
                (option, value)
            } else {
                let option = OPTIONS
                    .iter()
                    .find(|option| option.short == Some(bytes[letter]))
                    .ok_or_else(|| {
                        diagnostic(
                            &[b"invalid option -- '", &bytes[letter..letter + 1], b"'"],
                            name,
                        )
                    })?;
                let value =
                    (option.value && letter + 1 < bytes.len()).then_some(&bytes[letter + 1..]);
                (option, value)
            };
            let value = if option.value {
                match attached {
                    Some(value) => Some(value),
                    None => {
                        let arg = args.get(at).ok_or_else(|| {
                            if long {
                                diagnostic(
                                    &[b"option '--", option.long, b"' requires an argument"],
                                    name,
                                )
                            } else {
                                diagnostic(
                                    &[
                                        b"option requires an argument -- '",
                                        &bytes[letter..letter + 1],
                                        b"'",
                                    ],
                                    name,
                                )
                            }
                        })?;
                        at += 1;
                        Some(arg.as_bytes())
                    }
                }
            } else {
                None
            };
            options.controls.record(option.setting, option.long);
            match option.setting {
                Setting::Full => options.full = true,
                // These options have no effect in aggregation. Their table-mode
                // consumers belong to the respective mode implementations.
                Setting::TableOnly => options.strict = false,
                Setting::Filler => {
                    options.filler = Cow::Owned(copy_argument(value.unwrap())?.into_vec())
                }
                Setting::Vnlog => {
                    options.vnlog = true;
                    options.skip_comments = true;
                    options.header_in = true;
                    options.header_out = true;
                    options.filler = Cow::Borrowed(b"-");
                    options.input = Separator::Whitespace;
                    options.output = b' ';
                }
                Setting::ZeroTerminated => options.record_end = 0,
                Setting::Help => return Ok(Action::Help),
                Setting::Version => return Ok(Action::Version),
                Setting::Color => {
                    *color = match value.unwrap() {
                        b"auto" => ColorMode::Auto,
                        b"always" => ColorMode::Always,
                        b"never" => ColorMode::Never,
                        _ => {
                            return Err(failure(
                                b"invalid color mode; use auto, always or never\n".to_vec(),
                            ));
                        }
                    };
                }
                Setting::NoColor => *color = ColorMode::Never,
                Setting::HeaderIn => options.header_in = true,
                Setting::HeaderOut => {
                    options.header_out = true;
                    options.explicit_header_out = true;
                }
                Setting::Headers => {
                    options.header_in = true;
                    options.header_out = true;
                    options.explicit_header_out = true;
                }
                Setting::Comments => {
                    options.skip_comments = true;
                    options.controls.record_csv_input(option.long);
                }
                Setting::ResultName => options.result_names.add(value.unwrap())?,
                Setting::CsvOut => options.csv_out = true,
                Setting::CsvIn => options.csv_in = true,
                Setting::Csv => {
                    options.csv_in = true;
                    options.csv_out = true;
                }
                Setting::Na => options.narm = true,
                Setting::Sort => options.sort = true,
                Setting::IgnoreCase => options.ignore_case = true,
                Setting::Whitespace => {
                    options.controls.record_csv_input(option.long);
                    options.input = Separator::Whitespace;
                    options.output = b'\t';
                }
                Setting::Group => options.group = Some(copy_argument(value.unwrap())?),
                Setting::Seed => options.seed = Some(seed(value.unwrap())?),
                Setting::Format => options.presentation.format(value.unwrap())?,
                Setting::Round => options.presentation.round(value.unwrap())?,
                Setting::Unavailable => {
                    // Only this unsupported-setting path needs a classification
                    // pass: the mode may follow it. Reuse the same scanner so
                    // values, option permutations and -- retain their meaning.
                    let extended_mode = if allow_unavailable {
                        true
                    } else if let Ok(Action::Calculate(parsed)) =
                        // The classification pass can see later controls;
                        // only the outer scanner selects presentation policy.
                        parse_options(
                            args,
                            name,
                            posixly_correct,
                            locale,
                            true,
                            &mut ColorMode::Auto,
                        )
                    {
                        parsed
                            .operands
                            .first()
                            .is_some_and(|arg| matches!(arg.as_bytes(), b"health" | b"compare"))
                            || matches!(
                                super::grammar::requests_selection(&parsed.operands),
                                Ok(true)
                            )
                    } else {
                        false
                    };
                    if !extended_mode {
                        return Err(unavailable(bytes));
                    }
                    options.controls.record_unavailable(option.long);
                }
                Setting::Input | Setting::Output | Setting::Collapse => {
                    let [delimiter] = value.unwrap() else {
                        return Err(failure(
                            b"the delimiter must be a single character\n".to_vec(),
                        ));
                    };
                    match option.setting {
                        Setting::Input => {
                            options.controls.record_csv_input(option.long);
                            options.input = Separator::Literal(*delimiter);
                            options.output = *delimiter;
                        }
                        Setting::Output => {
                            options.supplied_output = true;
                            // GNU's signed char 0xff behaves as an unset override.
                            explicit_output = (*delimiter != 0xff).then_some(*delimiter);
                        }
                        Setting::Collapse => options.collapse = *delimiter,
                        _ => unreachable!(),
                    }
                }
            }
            if long || option.value || letter + 1 == bytes.len() {
                break;
            }
            letter += 1;
        }
    }
    if options.operands.is_empty() {
        return Err(diagnostic(&[b"missing operation specifiers"], name));
    }
    options.output = explicit_output.unwrap_or(options.output);
    options.explicit_output = explicit_output.is_some();
    if options.csv_out {
        options.output = b',';
    }
    Ok(Action::Calculate(Box::new(options)))
}

impl Options {
    /// Whether GNU's deprecation warning for `--full` applies: `--full` with
    /// Operations that are not Per-row operations.
    pub(super) fn warns_full(&self) -> bool {
        self.full && !self.linewise
    }

    /// Whether each Group of `-s` is exactly the records whose sort keys the
    /// sort compares equal, in input order, however the sort route brings
    /// them together, which hash grouping relies on: not with vnlog
    /// annotations or Per-row operations. With `-W` the sort keys keep the
    /// blanks before each field, which Groups ignore; hash grouping keys its
    /// classes by them and checks that no two differ only there. Every option
    /// is named, so that a new one is judged.
    pub(super) fn groups_are_sort_key_classes(&self) -> bool {
        let Self {
            group: _,
            operands: _,
            input: _,
            output: _,
            csv_out: _,
            csv_in: _,
            supplied_output: _,
            collapse: _,
            record_end: _,
            full: _,
            linewise,
            crosstab: _,
            strict: _,
            header_in: _,
            header_out: _,
            explicit_header_out: _,
            result_names: _,
            skip_comments: _,
            narm: _,
            vnlog,
            filler: _,
            explicit_output: _,
            sort: _,
            ignore_case: _,
            seed: _,
            presentation: _,
            locale: _,
            sort_memory: _,
            controls: _,
        } = self;
        !vnlog && !linewise
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    #[test]
    fn color_state_tracks_only_controls_consumed_by_the_outer_scan() {
        for (parts, posix, color, succeeds) in [
            (
                vec!["--color=always", "--help", "--no-color"],
                false,
                ColorMode::Always,
                true,
            ),
            (
                vec!["--help", "--color=always"],
                false,
                ColorMode::Auto,
                true,
            ),
            (
                vec!["--color=never", "--color=invalid", "--color=always"],
                false,
                ColorMode::Never,
                false,
            ),
            (vec!["-t", "--color=always"], false, ColorMode::Auto, false),
            (vec!["--", "--color=always"], false, ColorMode::Auto, true),
            (
                vec!["count", "1", "--color=always"],
                true,
                ColorMode::Auto,
                true,
            ),
            (
                vec!["count", "1", "--color=always"],
                false,
                ColorMode::Always,
                true,
            ),
            (
                vec![
                    "--color=never",
                    "--sort-cmd=/bin/sort",
                    "--color=always",
                    "count",
                    "1",
                ],
                false,
                ColorMode::Never,
                false,
            ),
            (
                vec![
                    "--color=never",
                    "--sort-cmd=/bin/sort",
                    "--color=always",
                    "health",
                ],
                false,
                ColorMode::Always,
                true,
            ),
        ] {
            let scan = scan_with_policy(&args(&parts), b"fastmash", posix, Default::default());
            assert_eq!(scan.color, color, "{parts:?}, posix={posix}");
            assert_eq!(scan.action.is_ok(), succeeds, "{parts:?}, posix={posix}");
        }
    }
    fn parsed(parts: &[&str]) -> Options {
        match parse(&args(parts), b"fastmash", false).ok().unwrap() {
            Action::Calculate(options) => *options,
            _ => panic!("expected calculation options"),
        }
    }

    #[test]
    fn seed_forms_parse_like_nonnegative_c_long_and_last_wins() {
        for (value, expected) in [
            ("", 0),
            ("0", 0),
            ("-0", 0),
            ("+0001", 1),
            ("  \t\n\x0b\x0c\r+1", 1),
            ("4294967297", 1),
            ("9223372036854775807", u32::MAX),
        ] {
            assert_eq!(seed(value.as_bytes()).ok().unwrap(), expected);
        }
        for parts in [
            vec!["-S7", "count", "1"],
            vec!["-S", "7", "count", "1"],
            vec!["--seed=7", "count", "1"],
            vec!["--seed", "7", "count", "1"],
        ] {
            assert_eq!(parsed(&parts).seed, Some(7));
        }
        assert_eq!(
            parsed(&["-S7", "--seed=4294967297", "count", "1"]).seed,
            Some(1)
        );
        assert_eq!(parsed(&["count", "1"]).seed, None);
    }

    #[test]
    fn seed_rejects_nonvalues_negative_values_overflow_and_trailing_bytes() {
        for value in [
            " ",
            "\t\r",
            "+",
            "-",
            "-1",
            "1 ",
            "1x",
            "9223372036854775808",
            "184467440737095516160",
            "\u{80}",
        ] {
            let error = seed(value.as_bytes()).unwrap_err();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"invalid seed\n");
        }
    }

    #[test]
    fn seed_required_values_keep_information_and_end_marker_ordering() {
        assert!(matches!(
            parse(&args(&["--help", "-Sbad"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert_eq!(
            parse(&args(&["-Sbad", "--help"]), b"fastmash", false)
                .err()
                .unwrap()
                .message,
            b"invalid seed\n"
        );
        assert_eq!(
            parse(&args(&["-S"]), b"fastmash", false)
                .err()
                .unwrap()
                .message,
            b"option requires an argument -- 'S'\nTry 'fastmash --help' for more information.\n"
        );
        assert_eq!(
            parse(&args(&["--seed"]), b"fastmash", false)
                .err()
                .unwrap()
                .message,
            b"option '--seed' requires an argument\nTry 'fastmash --help' for more information.\n"
        );
        assert_eq!(parsed(&["--", "-S7"]).operands, args(&["-S7"]));
    }

    #[test]
    fn collapse_delimiter_forms_repeat_and_stay_independent() {
        assert_eq!(parsed(&["unique", "1"]).collapse, b',');
        for parts in [
            vec!["-c:", "unique", "1"],
            vec!["-c", ":", "unique", "1"],
            vec!["--collapse-delimiter=:", "unique", "1"],
            vec!["--collapse-delimiter", ":", "unique", "1"],
        ] {
            assert_eq!(parsed(&parts).collapse, b':');
        }
        let options = parsed(&[
            "-t,",
            "--output-delimiter=|",
            "-c:",
            "--collapse-delimiter=;",
            "unique",
            "1",
        ]);
        assert_eq!(options.input, Separator::Literal(b','));
        assert_eq!(options.output, b'|');
        assert_eq!(options.collapse, b';');
    }

    #[test]
    fn collapse_delimiter_consumes_short_cluster_remainders() {
        let options = parsed(&["-Hc|", "unique", "name"]);
        assert!(options.header_in && options.header_out);
        assert_eq!(options.collapse, b'|');

        let options = parsed(&["-cH", "unique", "1"]);
        assert!(!options.header_in && !options.header_out);
        assert_eq!(options.collapse, b'H');
        assert_eq!(
            parse(&args(&["-c|H", "unique", "1"]), b"fastmash", false)
                .err()
                .unwrap()
                .message,
            b"the delimiter must be a single character\n"
        );
    }

    #[test]
    fn collapse_delimiter_validates_raw_byte_and_information_order() {
        for parts in [
            vec!["-c", "", "unique", "1"],
            vec!["-cxx", "unique", "1"],
            vec!["--collapse-delimiter=", "unique", "1"],
            vec!["--collapse-delimiter=é", "unique", "1"],
        ] {
            let error = parse(&args(&parts), b"fastmash", false).err().unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"the delimiter must be a single character\n");
        }
        assert_eq!(
            parse(&args(&["-c"]), b"cmd", false).err().unwrap().message,
            b"option requires an argument -- 'c'\nTry 'cmd --help' for more information.\n"
        );
        assert_eq!(
            parse(&args(&["--collapse-delimiter"]), b"cmd", false)
                .err()
                .unwrap()
                .message,
            b"option '--collapse-delimiter' requires an argument\nTry 'cmd --help' for more information.\n"
        );
        assert_eq!(
            parse(&args(&["--collapse-delim", "unique", "1"]), b"cmd", false)
                .err()
                .unwrap()
                .status,
            1
        );
        assert!(matches!(
            parse(&args(&["--help", "-c"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert_eq!(
            parse(&args(&["-c", "--help"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );

        let raw = vec![
            OsString::from_vec(vec![b'-', b'c', 0xff]),
            "unique".into(),
            "1".into(),
        ];
        let Action::Calculate(options) = parse(&raw, b"fastmash", false).ok().unwrap() else {
            panic!()
        };
        assert_eq!(options.collapse, 0xff);
    }

    #[test]
    fn narm_is_exact_repeatable_and_permuted() {
        assert!(!parsed(&["sum", "1"]).narm);
        let options = parsed(&["--narm", "-HWC", "sum", "a", "--narm"]);
        assert!(options.narm && options.header_in && options.skip_comments);
        assert_eq!(options.operands, args(&["sum", "a"]));
        assert_eq!(options.input, Separator::Whitespace);
        assert!(parsed(&["--nar", "sum", "1"]).narm);
        for (flag, status) in [("--narm=yes", 1), ("-R", 1)] {
            assert_eq!(
                parse(&args(&[flag, "sum", "1"]), b"fastmash", false)
                    .err()
                    .unwrap()
                    .status,
                status
            );
        }
    }

    #[test]
    fn narm_preserves_values_sentinel_and_information_precedence() {
        let options = parsed(&["--", "--narm", "sum", "1"]);
        assert!(!options.narm);
        assert_eq!(options.operands, args(&["--narm", "sum", "1"]));
        assert_eq!(
            parse(&args(&["-t", "--narm", "sum", "1"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
        assert!(matches!(
            parse(&args(&["--narm", "--help"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert!(matches!(
            parse(&args(&["--narm", "--version"]), b"fastmash", false),
            Ok(Action::Version)
        ));
        assert!(parse(&args(&["--narm=yes", "--help"]), b"fastmash", false).is_err());
        assert!(matches!(
            parse(&args(&["--help", "--narm=yes"]), b"fastmash", false),
            Ok(Action::Help)
        ));
    }

    #[test]
    fn comment_flags_repeat_cluster_and_preserve_delimiters() {
        assert!(!parsed(&["sum", "1"]).skip_comments);
        for flag in ["-C", "-CC", "--skip-comments"] {
            let options = parsed(&["-t,", "sum", "1", flag]);
            assert!(options.skip_comments);
            assert_eq!(options.input, Separator::Literal(b','));
            assert_eq!(options.output, b',');
        }
        for flag in ["-CWH", "-HWC", "-CHWCHW"] {
            let options = parsed(&[flag, "sum", "a"]);
            assert!(options.skip_comments && options.header_in && options.header_out);
            assert_eq!(options.input, Separator::Whitespace);
        }
        let options = parsed(&["--output-delimiter=|", "-CHWt,", "sum", "1"]);
        assert!(options.skip_comments && options.header_in && options.header_out);
        assert_eq!(options.input, Separator::Literal(b','));
        assert_eq!(options.output, b'|');
    }

    #[test]
    fn comment_flags_preserve_values_sentinel_and_information_precedence() {
        assert!(!parsed(&["-tC", "sum", "1"]).skip_comments);
        let options = parsed(&["-CtC", "sum", "1"]);
        assert!(options.skip_comments);
        assert_eq!(options.input, Separator::Literal(b'C'));
        let options = parsed(&["--", "-C", "sum", "1"]);
        assert!(!options.skip_comments);
        assert_eq!(options.operands, args(&["-C", "sum", "1"]));
        assert!(matches!(
            parse(&args(&["-C", "--help"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert!(parse(&args(&["-t", "-C", "--help"]), b"fastmash", false).is_err());
        assert!(parse(&args(&["-CX", "--help"]), b"fastmash", false).is_err());
        assert!(matches!(
            parse(&args(&["--help", "-CX"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert!(
            parse(
                &args(&["--skip-comments=yes", "sum", "1"]),
                b"fastmash",
                false
            )
            .is_err()
        );
    }

    #[test]
    fn whitespace_and_literal_modes_follow_last_input_option() {
        for prefix in [
            vec!["-W"],
            vec!["--whitespace"],
            vec!["-WW"],
            vec!["-t,", "-W"],
        ] {
            let mut args = prefix;
            args.extend(["sum", "1"]);
            let options = parsed(&args);
            assert_eq!(
                (options.input, options.output),
                (Separator::Whitespace, b'\t')
            );
        }
        for prefix in [vec!["-W", "-t,"], vec!["-Wt,"], vec!["-HWt,"]] {
            let mut args = prefix;
            args.extend(["sum", "1"]);
            let options = parsed(&args);
            assert_eq!(
                (options.input, options.output),
                (Separator::Literal(b','), b',')
            );
        }
        let options = parsed(&["--output-delimiter=|", "-W", "-t,", "sum", "1"]);
        assert_eq!(
            (options.input, options.output),
            (Separator::Literal(b','), b'|')
        );
        let options = parsed(&["-t,", "--output-delimiter=|", "-W", "sum", "1"]);
        assert_eq!(
            (options.input, options.output),
            (Separator::Whitespace, b'|')
        );
    }

    #[test]
    fn whitespace_clusters_preserve_required_values_and_end_marker() {
        for flag in ["-WH", "-HW", "-HWHW"] {
            let options = parsed(&[flag, "sum", "1"]);
            assert!(options.header_in && options.header_out);
            assert_eq!(options.input, Separator::Whitespace);
        }
        for flag in ["-tW", "-WtW"] {
            assert_eq!(parsed(&[flag, "sum", "1"]).input, Separator::Literal(b'W'));
        }
        assert_eq!(
            parsed(&["--", "sum", "1", "-W"]).operands,
            args(&["sum", "1", "-W"])
        );
        assert!(matches!(
            parse(&args(&["-W", "--help"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert_eq!(
            parse(&args(&["-t", "-W", "sum", "1"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
        assert_eq!(
            parse(&args(&["-WX", "sum", "1"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
    }

    #[test]
    fn aliases_repeat_and_permute_without_changing_operands() {
        for parts in [
            vec!["--headers", "sum", "1", "mean", "2"],
            vec!["sum", "-H", "1", "mean", "2"],
            vec!["-HHH", "sum", "1", "mean", "2", "--headers"],
            vec![
                "--header-out",
                "sum",
                "1",
                "--headers",
                "mean",
                "2",
                "--header-in",
            ],
        ] {
            let options = parsed(&parts);
            assert!(options.header_in && options.header_out);
            assert_eq!(options.operands, args(&["sum", "1", "mean", "2"]));
        }
    }

    #[test]
    fn header_clusters_stop_at_the_required_delimiter_value() {
        for parts in [vec!["-Ht,", "sum", "1"], vec!["-HHt", ",", "sum", "1"]] {
            let options = parsed(&parts);
            assert!(options.header_in && options.header_out);
            assert_eq!(
                (options.input, options.output),
                (Separator::Literal(b','), b',')
            );
        }
        let options = parsed(&["-tH", "sum", "1"]);
        assert!(!options.header_in && !options.header_out);
        assert_eq!(
            (options.input, options.output),
            (Separator::Literal(b'H'), b'H')
        );
        let options = parsed(&["-HtH", "--output-delimiter=|", "sum", "1"]);
        assert!(options.header_in && options.header_out);
        assert_eq!(
            (options.input, options.output),
            (Separator::Literal(b'H'), b'|')
        );
        let raw = vec![
            OsString::from_vec(b"-Ht\xff".to_vec()),
            "sum".into(),
            "1".into(),
        ];
        let Action::Calculate(options) = parse(&raw, b"fastmash", false).ok().unwrap() else {
            panic!()
        };
        assert!(options.header_in && options.header_out);
        assert_eq!(
            (options.input, options.output),
            (Separator::Literal(0xff), 0xff)
        );
    }

    #[test]
    fn aliases_preserve_information_sentinel_and_failure_policies() {
        assert!(matches!(
            parse(&args(&["-HH", "--help"]), b"cmd", false),
            Ok(Action::Help)
        ));
        assert!(matches!(
            parse(&args(&["--headers", "--version"]), b"cmd", false),
            Ok(Action::Version)
        ));
        let options = parsed(&["--", "-H", "sum", "1"]);
        assert!(!options.header_in && !options.header_out);
        assert_eq!(options.operands, args(&["-H", "sum", "1"]));
        for parts in [vec!["-Ht", "--help"], vec!["-t", "--headers"], vec!["-t,H"]] {
            let error = parse(&args(&parts), b"cmd", false).err().unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"the delimiter must be a single character\n");
        }
        let error = parse(&args(&["-HHt"]), b"cmd", false).err().unwrap();
        assert_eq!(error.status, 1);
        assert_eq!(
            error.message,
            b"option requires an argument -- 't'\nTry 'cmd --help' for more information.\n"
        );
        for option in ["-HX", "-H-H", "--headers=yes", "--head"] {
            let error = parse(&args(&[option, "--help"]), b"cmd", false)
                .err()
                .unwrap();
            assert_eq!(error.status, 1);
        }
        let error = parse(&args(&["-H"]), b"cmd", false).err().unwrap();
        assert_eq!(error.status, 1);
        assert_eq!(
            error.message,
            b"missing operation specifiers\nTry 'cmd --help' for more information.\n"
        );
    }

    #[test]
    fn output_header_permutation_and_precedence() {
        for parts in [
            vec!["--header-out", "sum", "1"],
            vec!["sum", "--header-out", "1", "--header-out"],
            vec!["--header-in", "sum", "1", "--header-out"],
        ] {
            let options = parsed(&parts);
            assert!(options.header_out);
            assert_eq!(options.operands, args(&["sum", "1"]));
        }
        assert!(!parsed(&["sum", "1"]).header_out);
        assert!(!parsed(&["--", "sum", "1", "--header-out"]).header_out);
        assert!(matches!(
            parse(&args(&["--header-out", "--help"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        let error = parse(
            &args(&["-t", "--header-out", "sum", "1"]),
            b"fastmash",
            false,
        )
        .err()
        .unwrap();
        assert_eq!(error.status, 1);
        assert!(parsed(&["--header-ou", "sum", "1"]).header_out);
        assert_eq!(
            parse(&args(&["--header-out=yes", "sum", "1"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
    }

    #[test]
    fn input_header_permutation_preserves_numeric_operands() {
        for parts in [
            vec!["--header-in", "sum", "1"],
            vec!["sum", "--header-in", "1"],
            vec!["sum", "1", "--header-in", "--header-in"],
        ] {
            let options = parsed(&parts);
            assert_eq!(options.operands, args(&["sum", "1"]));
            assert!(options.header_in);
        }
        assert!(!parsed(&["sum", "1"]).header_in);
    }

    #[test]
    fn input_header_respects_information_values_and_end_marker() {
        assert!(matches!(
            parse(&args(&["--header-in", "--help"]), b"fastmash", false),
            Ok(Action::Help)
        ));
        assert_eq!(
            parsed(&["--", "sum", "1", "--header-in"]).operands,
            args(&["sum", "1", "--header-in"])
        );
        let error = parse(
            &args(&["-t", "--header-in", "sum", "1"]),
            b"fastmash",
            false,
        )
        .err()
        .unwrap();
        assert_eq!(error.status, 1);
        assert_eq!(error.message, b"the delimiter must be a single character\n");
    }

    #[test]
    fn options_permute_without_reordering_operands() {
        for parts in [
            vec!["-t,", "sum", "1,2", "mean", "2"],
            vec!["sum", "-t", ",", "1,2", "mean", "2"],
            vec!["sum", "1,2", "mean", "2", "--field-separator=,"],
            vec!["--field-separator", ",", "sum", "1,2", "mean", "2"],
        ] {
            let options = parsed(&parts);
            assert_eq!(options.operands, args(&["sum", "1,2", "mean", "2"]));
            assert_eq!(
                (options.input, options.output),
                (Separator::Literal(b','), b',')
            );
        }
    }

    #[test]
    fn repeated_values_and_output_override_order() {
        for parts in [
            vec!["--output-delimiter", "|", "-t;", "-t,", "sum", "1"],
            vec![
                "-t,",
                "--output-delimiter=;",
                "--output-delimiter=|",
                "sum",
                "1",
            ],
        ] {
            let options = parsed(&parts);
            assert_eq!(
                (options.input, options.output),
                (Separator::Literal(b','), b'|')
            );
        }
    }

    #[test]
    fn high_byte_override_and_literal_controls() {
        for delimiter in [b' ', b'\t', b'\r', b'\n', 0x80, 0xff] {
            let mut input = args(&["-t"]);
            input.push(OsString::from_vec(vec![delimiter]));
            input.extend(args(&["sum", "1"]));
            let Action::Calculate(options) = parse(&input, b"fastmash", false).ok().unwrap() else {
                panic!("expected calculation options");
            };
            assert_eq!(
                (options.input, options.output),
                (Separator::Literal(delimiter), delimiter)
            );
        }
        let mut input = args(&["--output-delimiter=|", "-t,", "--output-delimiter"]);
        input.push(OsString::from_vec(vec![0xff]));
        input.extend(args(&["sum", "1"]));
        let Action::Calculate(options) = parse(&input, b"fastmash", false).ok().unwrap() else {
            panic!("expected calculation options");
        };
        assert_eq!(
            (options.input, options.output),
            (Separator::Literal(b','), b',')
        );
    }

    #[test]
    fn required_values_and_end_marker_have_distinct_roles() {
        let options = parsed(&["-t", "-", "sum", "1", "--", "-t,"]);
        assert_eq!(options.input, Separator::Literal(b'-'));
        assert_eq!(options.operands, args(&["sum", "1", "-t,"]));
        let options = parsed(&["--", "sum", "-1"]);
        let error = crate::grammar::parse(&options.operands).err().unwrap();
        assert_eq!(error.message, b"invalid field range for operation 'sum'\n");
        let error = parse(&args(&["-t", "--", "sum", "1"]), b"fastmash", false)
            .err()
            .unwrap();
        assert_eq!(error.message, b"the delimiter must be a single character\n");
    }

    #[test]
    fn invalid_values_and_options_precede_grammar() {
        for parts in [
            vec!["sum", "0", "-t", "::"],
            vec!["-t", "", "--unknown", "sum", "1"],
            vec!["-t", "\u{e9}", "sum", "1"],
            vec!["--output-delimiter=", "sum", "1"],
            vec!["-t::", "sum", "1"],
        ] {
            let error = parse(&args(&parts), b"fastmash", false).err().unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"the delimiter must be a single character\n");
        }
        let options = parsed(&["sum", "0", "-t,"]);
        assert_eq!(
            crate::grammar::parse(&options.operands)
                .err()
                .unwrap()
                .status,
            1
        );
        assert_eq!(
            parse(&args(&["sum", "0", "-WX"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
    }

    #[test]
    fn missing_values_render_the_supplied_command_name() {
        for (option, prefix) in [
            ("-t", "option requires an argument -- 't'\n"),
            (
                "--field-separator",
                "option '--field-separator' requires an argument\n",
            ),
            (
                "--output-delimiter",
                "option '--output-delimiter' requires an argument\n",
            ),
        ] {
            let error = parse(&args(&["sum", "1", option]), b"my-command", false)
                .err()
                .unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(
                error.message,
                format!("{prefix}Try 'my-command --help' for more information.\n").as_bytes()
            );
        }
    }

    #[test]
    fn first_informational_option_wins_before_operand_grammar() {
        for parts in [
            vec!["--help", "--version"],
            vec!["sum", "0", "--help"],
            vec!["sum", "-t,", "1", "--help"],
            vec!["--help", "-t", "xx"],
        ] {
            assert!(matches!(
                parse(&args(&parts), b"fastmash", false),
                Ok(Action::Help)
            ));
        }
        assert!(matches!(
            parse(&args(&["--version", "--help"]), b"fastmash", false),
            Ok(Action::Version)
        ));
    }

    #[test]
    fn earlier_option_errors_and_required_values_precede_information() {
        for parts in [
            vec!["-t", "--help"],
            vec!["--output-delimiter", "--version"],
            vec!["-t", "xx", "--help"],
        ] {
            let error = parse(&args(&parts), b"fastmash", false).err().unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"the delimiter must be a single character\n");
        }
        assert_eq!(
            parse(&args(&["-WX", "--help"]), b"fastmash", false)
                .err()
                .unwrap()
                .status,
            1
        );
    }

    #[test]
    fn end_marker_keeps_informational_spellings_as_operands() {
        let options = parsed(&["--", "--help", "--version"]);
        assert_eq!(options.operands, args(&["--help", "--version"]));
    }

    #[test]
    fn prefixes_and_posix_stopping_preserve_operands() {
        assert!(matches!(
            parse(&args(&["--hel"]), b"cmd", true),
            Ok(Action::Help)
        ));
        assert!(matches!(
            parse(&args(&["--vers"]), b"cmd", false),
            Ok(Action::Version)
        ));
        let result = parse(&args(&["count", "1", "-W"]), b"cmd", true)
            .ok()
            .unwrap();
        let Action::Calculate(options) = result else {
            panic!("calculation expected")
        };
        assert_eq!(options.operands, args(&["count", "1", "-W"]));
        assert_eq!(options.input, Separator::Literal(b'\t'));
        assert!(parse(&args(&[&"a".repeat(20_000), ""]), b"cmd", false).is_ok());
    }

    #[test]
    fn allocation_refusal_and_size_overflow_are_explicit() {
        let error = reserve(&mut Vec::<u8>::new(), usize::MAX).err().unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(error.message, b"command memory allocation failed\n");

        // Each reservation in operand vector growth, operand copies and group
        // storage is exercised independently; the hook resets on refusal.
        let input = args(&["-g", "1", "count", "2"]);
        for fail_after in 0..5 {
            FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
            let error = parse(&input, b"cmd", false).err().unwrap();
            assert_eq!(error.status, 77);
            assert_eq!(error.message, b"command memory allocation failed\n");
        }
        FAIL_RESERVATION.with(|slot| slot.set(Some(0)));
        assert_eq!(collect_args(args(&["cmd"])).err().unwrap().status, 77);
        assert!(parse(&input, b"cmd", false).is_ok());
    }
}
