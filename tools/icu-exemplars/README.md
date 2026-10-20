# ICU exemplar reference

This development tool reads the installed ICU 78 standard and auxiliary
exemplar sets for the locale names in a numeric reference JSON file. It follows
ICU resource inheritance and records `null` when the fallback language differs
from the requested language. Scalar ranges precede multicharacter strings,
which retain ICU's UTF-16 order.

Build separately from the application workspace. The build needs Rust 1.88 or
newer, ICU development files, pkg-config and libclang. The existing Python
producer also used the installed ICU 78 reference. Its runtime library is now
accessed through `rust_icu_ures`' safe API. The UnicodeSet parser handles literal
data, ranges and strings. Property expressions and variables are refused to
avoid substituting another Unicode release's property data. Third-party
libraries retain their own native implementations.

```sh
cargo run --locked --manifest-path tools/icu-exemplars/Cargo.toml --target-dir target -- \
  data/locales/glibc-lc-numeric.json /tmp/fastmash-icu-exemplars.json
```

The output must be new. A build against another ICU major version exits with
status 77 before creating output. Unsupported reference data also exits with
77. Invalid input and file errors exit with status 1. Successful output identifies
the ICU major version, and the console records the compiled full version.

`rust_icu_ures` and its bindings preserve the installed reference without
first-party FFI. `icu_unicodeset_parse` supplies a maintained parser. These
dependencies belong to this unpublished tool and do not enter the product or
ordinary workspace build. Its lockfile fixes the transitive versions. The
root license policy includes the two build-only permissive license exceptions.

Run its checks separately:

```sh
cargo fmt --manifest-path tools/icu-exemplars/Cargo.toml --check
cargo clippy --manifest-path tools/icu-exemplars/Cargo.toml --target-dir target --all-targets --locked -- -D warnings
cargo test --manifest-path tools/icu-exemplars/Cargo.toml --target-dir target --locked
cargo deny --manifest-path tools/icu-exemplars/Cargo.toml check
```

Tests cover inherited and unknown locales, string ordering, unsupported syntax,
output preservation and all retained independent exemplar arrays. CI also
builds with Ubuntu's ICU version and checks the required version refusal.
