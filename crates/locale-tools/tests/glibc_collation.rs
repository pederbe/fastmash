#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use fastmash_locale_tools::glibc_collation::{Batch, Pair, Reference};
use std::cmp::Ordering;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

fn pairs() -> Vec<Pair> {
    [
        ("a", "b"),
        ("é", "a"),
        ("a\tb", "a\nb"),
        ("", "x"),
        ("same", "same"),
    ]
    .map(|(left, right)| Pair {
        left: left.into(),
        right: right.into(),
    })
    .into()
}

#[test]
fn stable_opposite_orders_detect_ties_and_reject_corrupt_streams() {
    let pair = [Pair {
        left: "a".into(),
        right: "ä".into(),
    }];
    let batch = Batch::new(&pair).unwrap();
    assert_eq!(
        batch.decode("0a\x000ä\x001a\x001ä\x00".as_bytes()).unwrap(),
        [Ordering::Less]
    );
    assert_eq!(
        batch.decode("0ä\x000a\x001ä\x001a\x00".as_bytes()).unwrap(),
        [Ordering::Greater]
    );
    assert_eq!(
        batch.decode("0a\x000ä\x001ä\x001a\x00".as_bytes()).unwrap(),
        [Ordering::Equal]
    );
    for corrupt in [
        "0ä\x000a\x001a\x001ä\x00",
        "0a\x000ä\x001a\x001ä",
        "0a\x000ä\x001a\x001ä\x00extra\x00",
        "0a\x000ä\x001a\x001foreign\x00",
        "0a\x000ä\x002a\x002ä\x00",
    ] {
        assert!(batch.decode(corrupt.as_bytes()).is_err(), "{corrupt:?}");
    }
    assert!(
        Batch::new(&[Pair {
            left: "a\x00b".into(),
            right: "a".into()
        }])
        .is_err()
    );
}

#[test]
fn real_c_reference_preserves_utf8_tabs_newlines_empty_and_identical_payloads() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "glibc-pairs-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let mut pairs = pairs();
    // Exercise multi-digit group prefixes and the nonblocking large-input path.
    pairs.extend((0x80..0x300).map(|point| Pair {
        left: format!("\t{}\n", char::from_u32(point).unwrap()),
        right: format!("{}\t", char::from_u32(point + 1).unwrap()),
    }));
    let mut reference = Reference::open(
        std::path::Path::new("/usr/bin/sort"),
        std::path::Path::new("/usr/bin/locale"),
        &root,
        "C",
        &root.join("captures"),
    )
    .unwrap();
    let actual = reference.compare(&pairs).unwrap();
    assert!(
        Reference::open(
            std::path::Path::new("/usr/bin/sort"),
            std::path::Path::new("/usr/bin/locale"),
            &root,
            "not_a_locale.UTF-8",
            &root.join("missing")
        )
        .is_err()
    );
    assert_eq!(
        actual,
        pairs
            .iter()
            .map(|p| p.left.as_bytes().cmp(p.right.as_bytes()))
            .collect::<Vec<_>>()
    );
    fs::remove_dir_all(root).unwrap();
}
