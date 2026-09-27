//! Compare the general formatter with the separate exact-decimal presenter.
use fastmash_conversion::{
    format::{self, FormatId},
    presentation::Spec,
    profile::Profile,
};
use fastmash_numeric_contract::{Raw80, Value80};

fn check(raw: Raw80) {
    let value = Value80::from_raw(raw).unwrap();
    for profile in [Profile::C, Profile::DE_NUMERIC] {
        for (id, text) in [
            (FormatId::Default14, b"%.14g".as_slice()),
            (FormatId::Signed21, b"%+.21g".as_slice()),
        ] {
            let expected = Spec::parse(text).unwrap().render(value, profile).unwrap();
            let actual = format::format(value, profile, id, 16384).unwrap();
            assert_eq!(actual.bytes, expected, "{raw:?} {profile:?} {id:?}");
            assert_eq!(actual.input, actual.after);
        }
    }
}

#[test]
fn decimal_layout_across_binary80_range() {
    for exponent in (1..0x7fff)
        .step_by(127)
        .chain([1, 2, 0x3ffe, 0x3fff, 0x4000, 0x7ffe])
    {
        for significand in [1u64 << 63, (1u64 << 63) + 1, u64::MAX] {
            check(Raw80::new(exponent, significand));
            check(Raw80::new(exponent | 0x8000, significand));
        }
    }
    for significand in [0, 1, 2, (1u64 << 63) - 1] {
        check(Raw80::new(0, significand));
        check(Raw80::new(0x8000, significand));
    }
    // Exactly representable decimal powers and their immediate neighbors.
    for decimal in 0..=18 {
        let integer = 10u64.pow(decimal);
        let top = 63 - integer.leading_zeros();
        let exponent = (16383 + top) as u16;
        let significand = integer << (63 - top);
        check(Raw80::new(exponent, significand));
        check(Raw80::new(exponent, significand + 1));
        if significand > 1u64 << 63 {
            check(Raw80::new(exponent, significand - 1));
        } else {
            check(Raw80::new(exponent - 1, u64::MAX));
        }
    }
}

#[test]
#[ignore = "explicit formatter alternative measurement; not a timing assertion"]
fn compare_formatter_costs() {
    use std::{hint::black_box, time::Instant};
    let spec = Spec::parse(b"%.14g").unwrap();
    for (name, exponents, repetitions) in [
        ("ordinary", [0x3ffa, 0x3fff, 0x4005], 10000),
        ("extreme", [0, 1, 0x7ffe], 100),
    ] {
        let values: Vec<_> = exponents
            .into_iter()
            .map(|e| {
                Value80::from_raw(Raw80::new(
                    e,
                    if e == 0 { 1 } else { 0x9e37_79b9_7f4a_7c15 },
                ))
                .unwrap()
            })
            .collect();
        for value in &values {
            assert_eq!(
                format::format(*value, Profile::C, FormatId::Default14, 16384)
                    .unwrap()
                    .bytes,
                spec.render(*value, Profile::C).unwrap()
            );
        }
        for round in 0..7 {
            for offset in 0..2 {
                let alternative = (round + offset) % 2 == 1;
                let start = Instant::now();
                for _ in 0..repetitions {
                    for value in &values {
                        let bytes = if alternative {
                            spec.render(black_box(*value), Profile::C).unwrap()
                        } else {
                            format::format(
                                black_box(*value),
                                Profile::C,
                                FormatId::Default14,
                                16384,
                            )
                            .unwrap()
                            .bytes
                        };
                        black_box(bytes);
                    }
                }
                println!(
                    "{name}\t{round}\t{}\t{}\t{}",
                    if alternative {
                        "exact-expansion"
                    } else {
                        "corrected-search"
                    },
                    repetitions * values.len(),
                    start.elapsed().as_nanos()
                );
            }
        }
    }
}
