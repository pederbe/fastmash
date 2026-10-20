use crate::{Error, Result};
use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

pub const VERSION_CASES: &[&str] = &[
    "version",
    "version-locale",
    "version-first",
    "version-before-unknown",
    "output-header-reference:version-precedence",
    "header-aliases:alias-then-version",
    "named-fields:version",
    "whitespace:version",
    "skip-comments:version",
    "narm:version",
    "count:version",
];
pub const NATIVE_SORT_DIAGNOSTICS: &[&str] =
    &["sorting/header-empty", "sorting/header-only-comments"];
pub const TRANSPORT_REASON: &str =
    "non-ordinary input transport; covered by the full Python corpus run";
pub const FILE_TRANSPORT_REASON: &str =
    "transport case is covered by the default Rust run, not --stdin-file";
pub const NATIVE_REASON: &str =
    "native missing-header diagnostic replaces Linux system-sort stderr";

#[derive(Clone, Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub io: String,
    #[serde(default)]
    pub observe_held: serde_json::Value,
    #[serde(default)]
    pub hold_stdin: bool,
    pub args: Option<Vec<Argument>>,
    pub args_hex: Option<Vec<String>>,
    pub input_hex: String,
    pub locale: String,
    pub posixly_correct: Option<String>,
    pub name: Option<String>,
    pub expected: FrozenExpected,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FrozenExpected {
    pub returncode: i32,
    pub stdout_hex: Option<String>,
    pub stdout_file: Option<PathBuf>,
    pub stderr_hex: String,
    // Transport expectations remain in the frozen fixture and are deferred,
    // rather than weakened to make them fit the ordinary command interface.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Expected {
    pub returncode: i32,
    pub stdout_hex: String,
    pub stderr_hex: String,
}

#[derive(Clone, Debug)]
pub struct Argument(Vec<u8>);

impl<'de> Deserialize<'de> for Argument {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ArgumentVisitor;
        impl de::Visitor<'_> for ArgumentVisitor {
            type Value = Argument;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a command argument with Python surrogateescape bytes")
            }
            fn visit_bytes<E: de::Error>(
                self,
                value: &[u8],
            ) -> std::result::Result<Self::Value, E> {
                // serde_json's byte-string reader preserves lone UTF-16
                // surrogates as WTF-8. Python's Unix filesystem encoder maps
                // only U+DC80..U+DCFF back to their original raw byte.
                let mut bytes = Vec::new();
                let mut start = 0;
                let mut at = 0;
                while at + 2 < value.len() {
                    if value[at] == 0xed
                        && matches!(value[at + 1], 0xb2 | 0xb3)
                        && (0x80..=0xbf).contains(&value[at + 2])
                    {
                        std::str::from_utf8(&value[start..at]).map_err(E::custom)?;
                        bytes.extend_from_slice(&value[start..at]);
                        bytes.push(((value[at + 1] & 3) << 6) | (value[at + 2] & 63));
                        at += 3;
                        start = at;
                    } else {
                        at += 1;
                    }
                }
                std::str::from_utf8(&value[start..]).map_err(E::custom)?;
                bytes.extend_from_slice(&value[start..]);
                Ok(Argument(bytes))
            }
        }
        deserializer.deserialize_bytes(ArgumentVisitor)
    }
}

#[derive(Deserialize)]
struct Provenance {
    cases: usize,
    fixture_sha256: String,
    content_sha256: String,
}

pub struct Fixture {
    pub cases: Vec<Case>,
    pub path: PathBuf,
    pub provenance_path: PathBuf,
    pub fixture_sha256: String,
    pub content_sha256: String,
    pub provenance_sha256: String,
}

impl Fixture {
    pub fn load(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::message("fixture must have a UTF-8 filename"))?;
        let provenance_path = path.with_file_name(name.replace(".jsonl.gz", "-provenance.json"));
        let provenance_bytes = fs::read(&provenance_path)?;
        let provenance: Provenance = serde_json::from_slice(&provenance_bytes)?;
        let compressed = fs::read(&path)?;
        let mut content = Vec::new();
        MultiGzDecoder::new(compressed.as_slice()).read_to_end(&mut content)?;
        let fixture_sha256 = digest(&compressed);
        let content_sha256 = digest(&content);
        if fixture_sha256 != provenance.fixture_sha256
            || content_sha256 != provenance.content_sha256
        {
            return Err(Error::message(format!(
                "{} differs from its provenance",
                path.display()
            )));
        }
        let cases: Vec<Case> = content
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(serde_json::from_slice)
            .collect::<std::result::Result<_, _>>()?;
        let ids: HashSet<_> = cases.iter().map(|case| &case.id).collect();
        if cases.len() != provenance.cases || ids.len() != cases.len() {
            return Err(Error::message(format!(
                "{} has an incomplete or duplicated inventory",
                path.display()
            )));
        }
        Ok(Self {
            cases,
            path,
            provenance_path,
            fixture_sha256,
            content_sha256,
            provenance_sha256: digest(&provenance_bytes),
        })
    }
}

impl Case {
    pub fn ordinary(&self) -> bool {
        self.io == "normal" && !truthy(&self.observe_held) && !self.hold_stdin
    }

    pub fn supported(&self, stdin_file: bool) -> bool {
        self.ordinary()
            || (!stdin_file
                && matches!(self.io.as_str(), "directory" | "closed-pipe")
                && !truthy(&self.observe_held)
                && !self.hold_stdin)
    }

    pub fn disposition(&self, stdin_file: bool) -> &'static str {
        if !self.supported(stdin_file) {
            "deferred-transport"
        } else {
            match self.io.as_str() {
                "directory" => "directory-input",
                "closed-pipe" => "closed-output-pipe",
                _ if stdin_file => "ordinary-file",
                _ => "ordinary-pipe",
            }
        }
    }

    pub fn deferred_reason(&self, stdin_file: bool) -> Option<&'static str> {
        if self.supported(stdin_file) {
            None
        } else if self.supported(false) {
            Some(FILE_TRANSPORT_REASON)
        } else {
            Some(TRANSPORT_REASON)
        }
    }

    pub fn arguments(&self) -> Result<Vec<OsString>> {
        match (&self.args_hex, &self.args) {
            (Some(args), _) => args
                .iter()
                .map(|s| decode(s).map(OsString::from_vec))
                .collect(),
            (None, Some(args)) => Ok(args
                .iter()
                .map(|arg| OsString::from_vec(arg.0.clone()))
                .collect()),
            (None, None) => Err(Error::message(format!("{} has no arguments", self.id))),
        }
    }

    pub fn expected(&self, root: &Path, version: Option<&str>) -> Result<Expected> {
        if !self.supported(false) || !self.expected.other.is_empty() {
            return Err(Error::message(format!(
                "{} is not a supported expectation",
                self.id
            )));
        }
        let mut stdout = match (&self.expected.stdout_file, &self.expected.stdout_hex) {
            (Some(path), _) => fs::read(root.join(path))?,
            (None, Some(value)) => decode(value)?,
            (None, None) => {
                return Err(Error::message(format!(
                    "{} has no stdout expectation",
                    self.id
                )));
            }
        };
        if let Some(version) = version.filter(|_| VERSION_CASES.contains(&self.id.as_str())) {
            stdout = format!("fastmash {version}\n").into_bytes();
        }
        let stderr = if NATIVE_SORT_DIAGNOSTICS.contains(&self.id.as_str()) {
            b"fastmash: missing input header for named grouping key\n".to_vec()
        } else {
            decode(&self.expected.stderr_hex)?
        };
        Ok(Expected {
            returncode: self.expected.returncode,
            stdout_hex: encode(&stdout),
            stderr_hex: encode(&stderr),
        })
    }

    pub fn native_dispositions(&self) -> Vec<&'static str> {
        if NATIVE_SORT_DIAGNOSTICS.contains(&self.id.as_str()) {
            vec![NATIVE_REASON]
        } else {
            Vec::new()
        }
    }
}

// Python's ordinary predicate tests the truth value of observe_held. Keep its
// frozen false/null/empty-object meaning instead of treating mere presence as held.
fn truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Array(value) => !value.is_empty(),
        serde_json::Value::Object(value) => !value.is_empty(),
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Number(value) => value.as_f64() != Some(0.0),
    }
}

pub fn digest(bytes: &[u8]) -> String {
    encode(&Sha256::digest(bytes))
}

pub fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 15)]));
    }
    text
}

pub fn decode(text: &str) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(text.len() / 2);
    let mut digits = text.bytes().filter(|b| !b.is_ascii_whitespace());
    while let Some(high) = digits.next() {
        let low = digits
            .next()
            .ok_or_else(|| Error::message("odd-length hexadecimal bytes"))?;
        let high = char::from(high)
            .to_digit(16)
            .ok_or_else(|| Error::message("invalid hexadecimal bytes"))?;
        let low = char::from(low)
            .to_digit(16)
            .ok_or_else(|| Error::message("invalid hexadecimal bytes"))?;
        result.push((high * 16 + low) as u8);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Fixture {
        Fixture::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/cli/cases-v3.jsonl.gz"),
        )
        .unwrap()
    }

    #[test]
    fn frozen_inventory_preserves_all_dispositions_and_byte_arguments() {
        let fixture = fixture();
        assert_eq!(fixture.cases.len(), 3137);
        let ordinary: Vec<_> = fixture
            .cases
            .iter()
            .filter(|case| case.ordinary())
            .collect();
        assert_eq!(ordinary.len(), 2857);
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.supported(false))
                .count(),
            2930
        );
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.supported(true))
                .count(),
            2857
        );
        assert_eq!(
            fixture.cases.iter().filter(|case| !case.ordinary()).count(),
            280
        );
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| !case.ordinary() && case.io == "normal")
                .count(),
            68
        );
        assert_eq!(
            ordinary
                .iter()
                .filter(|case| case.args_hex.is_some())
                .count(),
            37
        );
        assert_eq!(
            ordinary
                .iter()
                .filter(|case| case.expected.stdout_file.is_some())
                .count(),
            10
        );
        for case in ordinary {
            case.arguments().unwrap();
            decode(&case.input_hex).unwrap();
        }
        use std::os::unix::ffi::OsStrExt;
        let high_byte = fixture
            .cases
            .iter()
            .find(|case| case.id == "collapse/high-byte-delimiter")
            .unwrap();
        assert_eq!(high_byte.arguments().unwrap()[0].as_bytes(), b"-c\xff");
    }

    #[test]
    fn argument_strings_preserve_unicode_and_only_python_surrogateescape_bytes() {
        for (json, bytes) in [
            (r#""-c\udcff""#, b"-c\xff".as_slice()),
            (r#""\udc80\udcff""#, b"\x80\xff".as_slice()),
            (r#""\ud801\udc00""#, "\u{10400}".as_bytes()),
            (r#""\\udcff""#, b"\\udcff".as_slice()),
            (r#""\u03b1""#, "α".as_bytes()),
        ] {
            assert_eq!(serde_json::from_str::<Argument>(json).unwrap().0, bytes);
        }
        for json in [r#""\udc7f""#, r#""\ud800""#, r#""\udd00""#] {
            assert!(serde_json::from_str::<Argument>(json).is_err());
        }
    }

    #[test]
    fn ordinary_expectations_only_apply_the_existing_named_adaptations() {
        let fixture = fixture();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for case in fixture.cases.iter().filter(|case| case.ordinary()) {
            let original = case.expected(&root, None).unwrap();
            let versioned = case.expected(&root, Some("development")).unwrap();
            assert_eq!(original.returncode, versioned.returncode);
            assert_eq!(original.stderr_hex, versioned.stderr_hex);
            if VERSION_CASES.contains(&case.id.as_str()) {
                assert_eq!(
                    decode(&versioned.stdout_hex).unwrap(),
                    b"fastmash development\n"
                );
            } else {
                assert_eq!(original.stdout_hex, versioned.stdout_hex, "{}", case.id);
            }
            if NATIVE_SORT_DIAGNOSTICS.contains(&case.id.as_str()) {
                assert_eq!(
                    decode(&original.stderr_hex).unwrap(),
                    b"fastmash: missing input header for named grouping key\n"
                );
                assert_eq!(original.returncode, 1);
                assert_eq!(
                    Some(&original.stdout_hex),
                    case.expected.stdout_hex.as_ref()
                );
            } else {
                assert_eq!(original.stderr_hex, case.expected.stderr_hex, "{}", case.id);
            }
        }
    }
}
