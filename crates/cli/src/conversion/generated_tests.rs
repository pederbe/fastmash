//! Compile the locale emitter's exact fixture against the product's Profile.
#[rustfmt::skip]
#[path = "testdata/generated_numeric.rs"]
mod generated;

type NumericProfile<'a> = (&'a [u8], u8, &'a [u8], &'a [u8]);

#[test]
fn generated_numeric_profiles_keep_their_bytes_and_membership() {
    let actual: Vec<_> = generated::NUMERIC
        .iter()
        .map(|(name, profile)| {
            (
                *name,
                profile.radix(),
                profile.thousands(),
                profile.grouping(),
            )
        })
        .collect();
    let expected: &[NumericProfile<'_>] = &[
        (b"aa_ER", b'.', b"", b""),
        (b"back\\slash", b'.', b"", b""),
        (b"bad\");compile_error!(\"injected", b'.', b"", b""),
        (b"carriage\rreturn", b'.', b"", b""),
        (b"de_DE", b',', b".", b"\x03\x03\x00"),
        ("fr_é".as_bytes(), b'.', b"", b""),
        (b"line\nbreak", b'.', b"", b""),
        (b"nan_TW@latin", b'.', b"\xc2\xa0", b"\x03\x00"),
        (b"quote\"name", b'.', b"", b""),
        (b"sr_RS@latin", b',', b".", b"\x03\x7f"),
    ];
    assert_eq!(actual, expected);
    assert_eq!(
        generated::BARE_UTF8,
        &[
            b"aa_ER".as_slice(),
            b"back\\slash",
            b"bad\");compile_error!(\"injected",
            b"carriage\rreturn",
            "fr_é".as_bytes(),
            b"line\nbreak",
            b"quote\"name",
            b"sr_RS@latin",
        ]
    );
    assert_eq!(
        generated::REFUSED,
        &[
            b"back\\slash@refused".as_slice(),
            b"bad\");compile_error!(\"injected@refused",
            b"carriage\rreturn@refused",
            b"fa_IR",
            "fr_é@refused".as_bytes(),
            b"line\nbreak@refused",
            b"quote\"name@refused",
        ]
    );
}
