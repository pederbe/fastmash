//! Command preparation preserves failure order and pre-input side effects.
use super::command_test_support::{self, Input, Output, Sources};
use super::{Environment, locale};

fn input() -> Input<'static> {
    Input {
        bytes: b"untouched",
        segment: 2,
        error: Some(5),
    }
}

fn output() -> Output {
    Output {
        bytes: Vec::new(),
        error: None,
        closed: false,
    }
}

#[test]
fn restricted_controls_keep_the_first_explicit_option() {
    for (args, status, diagnostic) in [
        (
            vec!["--filler=-", "--vnlog", "health"],
            1,
            &b"health does not support calculation option '--filler'\n"[..],
        ),
        (
            vec!["--round=14", "--round=14", "--seed=0", "top:1", "1"],
            77,
            &b"Top-N selection does not support --round\n"[..],
        ),
        (
            vec!["--whitespace", "-t", "\t", "--csv-in", "sum", "1"],
            1,
            &b"CSV input conflicts with --whitespace\n"[..],
        ),
        (
            vec!["-t", "\t", "--whitespace", "--csv-in", "sum", "1"],
            1,
            &b"CSV input conflicts with --field-separator\n"[..],
        ),
        (
            vec!["--csv-out", "--output-delimiter=\t", "noop"],
            1,
            &b"CSV output conflicts with --output-delimiter\n"[..],
        ),
    ] {
        let mut reader = input();
        let mut writer = output();
        let result = command_test_support::command(&mut reader, &mut writer, &args);
        assert_eq!(result, (status, vec![diagnostic.to_vec()]), "{args:?}");
        assert_eq!(reader.bytes, b"untouched");
        assert!(writer.bytes.is_empty());
    }
    let mut sources = Sources {
        inputs: [input(), input()],
        closes: [None, None],
        opened: 0,
        completed: 0,
    };
    let mut writer = output();
    let result = command_test_support::command_sources(
        &mut sources,
        &mut writer,
        &[
            "--collapse-delimiter=,",
            "--seed=0",
            "compare",
            "before",
            "after",
            "sum",
            "1",
        ],
    );
    assert_eq!(
        result,
        (
            77,
            vec![b"compare does not support --collapse-delimiter\n".to_vec()]
        )
    );
    assert_eq!((sources.opened, sources.completed), (0, 0));
    assert!(writer.bytes.is_empty());
}

#[test]
fn information_short_circuits_restricted_controls_without_input() {
    for args in [
        vec!["--round=14", "--help", "health"],
        vec!["--seed=0", "--version", "compare"],
    ] {
        let mut reader = input();
        let mut writer = output();
        assert_eq!(
            command_test_support::command(&mut reader, &mut writer, &args),
            (0, vec![])
        );
        assert_eq!(reader.bytes, b"untouched");
        assert!(!writer.bytes.is_empty());
    }
}

#[test]
fn scanner_errors_precede_restricted_control_rejections() {
    let diagnostic = b"invalid seed\n";
    let mut reader = input();
    let mut writer = output();
    let result = command_test_support::command(
        &mut reader,
        &mut writer,
        &["--filler=-", "--seed=bad", "health"],
    );
    assert_eq!(result, (1, vec![diagnostic.to_vec()]));
    assert_eq!(reader.bytes, b"untouched");
    assert!(writer.bytes.is_empty());

    let mut sources = Sources {
        inputs: [input(), input()],
        closes: [None, None],
        opened: 0,
        completed: 0,
    };
    let result = command_test_support::command_sources(
        &mut sources,
        &mut writer,
        &[
            "--collapse-delimiter=,",
            "--seed=bad",
            "compare",
            "before",
            "after",
            "sum",
            "1",
        ],
    );
    assert_eq!(result, (1, vec![diagnostic.to_vec()]));
    assert_eq!((sources.opened, sources.completed), (0, 0));
    assert!(writer.bytes.is_empty());
}

fn refused_numbers(sort_memory: Option<&str>) -> Environment {
    Environment {
        locale: locale::Policy::resolve(|key| (key == "LC_NUMERIC").then(|| "ps_AF.UTF-8".into())),
        posixly_correct: false,
        grouping: None,
        sort_memory: sort_memory.map(Into::into),
        pipe_grouping: None,
        terminal: Default::default(),
    }
}

#[test]
fn named_fields_keep_calculation_and_selection_locale_precedence() {
    for args in [vec!["sum", "named"], vec!["top:1", "named"]] {
        let mut reader = input();
        let mut writer = output();
        let (status, diagnostics) = command_test_support::command_in(
            &mut reader,
            &mut writer,
            &args,
            refused_numbers(None),
        );
        if args[0] == "sum" {
            assert_eq!(status, 77);
            assert_eq!(diagnostics.len(), 1);
            assert!(diagnostics[0].starts_with(b"unsupported numeric locale "));
        } else {
            assert_eq!(
                (status, diagnostics),
                (
                    1,
                    vec![b"-H or --header-in must be used with named columns\n".to_vec(),]
                )
            );
        }
        assert_eq!(reader.bytes, b"untouched");
        assert!(writer.bytes.is_empty());
    }
}

#[test]
fn ordinary_binding_allocation_precedes_named_header_prerequisites() {
    let args = ["count", "named"];
    let prefix = command_test_support::scanning_reservations(&args);
    for refuse in [true, false] {
        let mut reader = input();
        let mut writer = output();
        super::command_memory::FAIL_RESERVATION.with(|slot| slot.set(refuse.then_some(prefix)));
        let result = command_test_support::command(&mut reader, &mut writer, &args);
        super::command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        let expected = if refuse {
            (77, b"command memory allocation failed\n".as_slice())
        } else {
            (
                1,
                b"-H or --header-in must be used with named columns\n".as_slice(),
            )
        };
        assert_eq!(result, (expected.0, vec![expected.1.to_vec()]));
        assert_eq!(reader.bytes, b"untouched");
        assert!(writer.bytes.is_empty());
        assert!(!writer.closed);
    }
}

#[test]
fn selection_admits_sort_memory_before_numbers_but_calculations_do_not() {
    for operation in ["sum", "top:1"] {
        let mut reader = input();
        let mut writer = output();
        let (status, diagnostics) = command_test_support::command_in(
            &mut reader,
            &mut writer,
            &["-s", "-g", "1", operation, "2"],
            refused_numbers(Some("bad")),
        );
        assert_eq!(status, 77);
        if operation == "sum" {
            assert_eq!(diagnostics.len(), 1);
            assert!(diagnostics[0].starts_with(b"unsupported numeric locale "));
        } else {
            assert_eq!(
                diagnostics,
                vec![b"FASTMASH_SORT_MEMORY_BYTES must be a positive byte count\n".to_vec(),]
            );
        }
        assert_eq!(reader.bytes, b"untouched");
        assert!(writer.bytes.is_empty());
    }
}

#[test]
fn rmdup_buffer_failure_precedes_named_header_prerequisites() {
    struct RefusedBuffer;
    impl std::io::Write for RefusedBuffer {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            panic!("output after buffer refusal");
        }
        fn flush(&mut self) -> std::io::Result<()> {
            panic!("flush after buffer refusal");
        }
    }
    impl super::command_output::Transport for RefusedBuffer {
        fn buffering(&self) -> (usize, bool) {
            (0, false)
        }
        fn close(&mut self) -> std::io::Result<()> {
            panic!("close after buffer refusal");
        }
    }
    let mut reader = input();
    assert_eq!(
        command_test_support::command(&mut reader, &mut RefusedBuffer, &["rmdup", "named"]),
        (77, vec![b"unsupported output I/O error\n".to_vec()])
    );
    assert_eq!(reader.bytes, b"untouched");

    let mut writer = output();
    assert_eq!(
        command_test_support::command(&mut reader, &mut writer, &["rmdup", "named"]),
        (
            1,
            vec![b"-H or --header-in must be used with named columns\n".to_vec()]
        )
    );
    assert_eq!(reader.bytes, b"untouched");
    assert!(writer.bytes.is_empty());
    assert!(writer.closed);
}
