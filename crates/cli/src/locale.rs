//! Invocation-owned, pinned locale semantics; never consult host locale data.
use super::{Failure, unsupported};
use fastmash_conversion::profile::Profile;
use icu_collator::{
    Collator, CollatorBorrowed,
    options::{AlternateHandling, CollatorOptions, Strength},
};
use icu_locale_core::Locale;
use std::{ffi::OsString, os::unix::ffi::OsStrExt};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Collation {
    #[default]
    Bytes,
    /// Unicode collation for this BCP 47 locale, verified against glibc.
    Language(&'static str),
    Unsupported,
}
#[derive(Clone, Copy)]
pub(super) struct Policy {
    /// Number rules; `C` placeholder when `numbers` is false.
    pub numeric: Profile,
    numbers: bool,
    pub utf8: bool,
    /// Whether `-i` can fold ASCII letters: glibc's Turkic character types
    /// leave `i` and `I` unfolded.
    ascii_case: bool,
    pub collation: Collation,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            numeric: Profile::C,
            numbers: true,
            utf8: false,
            ascii_case: true,
            collation: Collation::Bytes,
        }
    }
}
fn recognized(bytes: &[u8]) -> Option<(bool, Profile, Collation)> {
    if let b"C" | b"POSIX" = bytes {
        return Some((false, Profile::C, Collation::Bytes));
    }
    if bytes.contains(&b'@') {
        return None;
    }
    let name = match bytes.iter().position(|&b| b == b'.') {
        Some(dot) if utf8_codeset(bytes) => &bytes[..dot],
        Some(_) => return None,
        // glibc locales whose default character set is UTF-8, such as hi_IN.
        None if super::numeric_locales::BARE_UTF8
            .binary_search(&bytes)
            .is_ok() =>
        {
            bytes
        }
        None => return None,
    };
    if name == b"C" {
        return Some((true, Profile::C, Collation::Bytes));
    }
    // Number rules of every glibc UTF-8 locale with a one-byte decimal point.
    let table = super::numeric_locales::NUMERIC;
    let numeric = table
        .binary_search_by(|(entry, _)| (*entry).cmp(name))
        .ok()
        .map(|at| table[at].1)?;
    // Locales whose letter order the Unicode collator reproduces.
    let table = super::collation_locales::COLLATION;
    let collation = table
        .binary_search_by(|(entry, _)| (*entry).cmp(name))
        .map_or(Collation::Unsupported, |at| {
            Collation::Language(table[at].1)
        });
    Some((true, numeric, collation))
}
/// Locales whose glibc `toupper('i')` and `tolower('I')` return them
/// unchanged (dotted and dotless i): checked for every compiled locale in
/// data/locales/glibc-lc-numeric.json, plus tt_RU@iqtelif, whose LC_CTYPE
/// copies tr_TR in glibc's localedata.
fn turkic_case(name: &[u8]) -> bool {
    let mut parts = name.splitn(2, |&b| b == b'@');
    let base = parts.next().unwrap_or_default();
    let modifier = parts.next();
    let language = base.split(|&b| b == b'.').next().unwrap_or_default();
    matches!(
        language,
        b"az_AZ" | b"crh_UA" | b"ku_TR" | b"tr_CY" | b"tr_TR"
    ) || (language == b"tt_RU" && modifier == Some(b"iqtelif"))
}
/// Whether a codeset-qualified locale name such as `nb_NO.UTF-8@euro` names UTF-8.
fn utf8_codeset(name: &[u8]) -> bool {
    let name = name.split(|&b| b == b'@').next().unwrap_or_default();
    name.iter().position(|&b| b == b'.').is_some_and(|dot| {
        let codeset = &name[dot + 1..];
        codeset.eq_ignore_ascii_case(b"UTF-8") || codeset.eq_ignore_ascii_case(b"utf8")
    })
}
/// The variable that sets a category, and its value: the first nonempty of
/// `LC_ALL`, the category and `LANG`, else `C`.
fn setting(
    get: &impl Fn(&str) -> Option<OsString>,
    category: &'static str,
) -> (&'static str, OsString) {
    ["LC_ALL", category, "LANG"]
        .into_iter()
        .find_map(|key| get(key).filter(|v| !v.is_empty()).map(|v| (key, v)))
        .unwrap_or(("", "C".into()))
}
/// `hint` names the variable to set as `{}`.
fn refusal(category: &'static str, reason: &str, hint: &str, utf8: bool) -> Failure {
    let (source, value) = setting(&|key| std::env::var_os(key), category);
    let variable = if source == "LC_ALL" {
        "LC_ALL"
    } else {
        category
    };
    let mut message = reason.as_bytes().to_vec();
    super::named_fields::quote_with(value.as_bytes(), &mut message, utf8);
    message.extend_from_slice(
        format!(" from {source}\nhint: {}\n", hint.replace("{}", variable)).as_bytes(),
    );
    Failure {
        status: 77,
        message,
    }
}
impl Policy {
    pub fn from_env() -> Self {
        Self::resolve(|key| std::env::var_os(key))
    }
    fn resolve(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let category = |name| setting(&get, name).1;
        // Messages are always English, and the character type selects the
        // quotation style and whether -i can fold ASCII letters; neither
        // category refuses a command by itself.
        let ctype = category("LC_CTYPE");
        let utf8 =
            recognized(ctype.as_bytes()).map_or_else(|| utf8_codeset(ctype.as_bytes()), |v| v.0);
        let numeric = recognized(category("LC_NUMERIC").as_bytes()).map(|v| v.1);
        let collation =
            recognized(category("LC_COLLATE").as_bytes()).map_or(Collation::Unsupported, |v| v.2);
        Self {
            numeric: numeric.unwrap_or(Profile::C),
            numbers: numeric.is_some(),
            utf8,
            ascii_case: !turkic_case(ctype.as_bytes()),
            collation,
        }
    }
    /// Refuses a command that reads or prints numbers under a numeric locale
    /// whose decimal separator Fastmash does not know.
    pub fn numbers(self) -> Result<(), Failure> {
        if self.numbers {
            return Ok(());
        }
        Err(refusal(
            "LC_NUMERIC",
            "unsupported numeric locale ",
            "set {}=C.UTF-8, or a UTF-8 locale whose decimal separator is one byte",
            self.utf8,
        ))
    }
    pub fn sorting(self) -> Result<(), Failure> {
        if self.collation != Collation::Unsupported {
            return Ok(());
        }
        Err(refusal(
            "LC_COLLATE",
            "unsupported sorting locale ",
            "set {}=C.UTF-8 for byte order; fastmash --help lists the language locales",
            self.utf8,
        ))
    }
    /// Refuses `-i` where glibc's case mapping is not ASCII's.
    pub fn case_folding(self) -> Result<(), Failure> {
        if self.ascii_case {
            return Ok(());
        }
        Err(refusal(
            "LC_CTYPE",
            "unsupported -i under character type ",
            "this locale does not fold i and I; set {}=C.UTF-8 or omit -i",
            self.utf8,
        ))
    }
    pub fn language(self) -> bool {
        matches!(self.collation, Collation::Language(_))
    }
    pub fn collator(self) -> Result<Option<CollatorBorrowed<'static>>, Failure> {
        self.sorting()?;
        let Collation::Language(tag) = self.collation else {
            return Ok(None);
        };
        let language = Locale::try_from_str(tag).expect("collation tags are valid BCP 47");
        let mut options = CollatorOptions::default();
        options.alternate_handling = Some(AlternateHandling::Shifted);
        options.strength = Some(Strength::Quaternary);
        Collator::try_new(language.into(), options)
            .map(Some)
            .map_err(|_| unsupported("compiled locale data unavailable"))
    }
}
pub(super) fn text(bytes: &[u8]) -> Result<&str, Failure> {
    if bytes.contains(&0) {
        return Err(unsupported(
            "language sorting requires NUL-free UTF-8 keys; use LC_COLLATE=C for raw bytes",
        ));
    }
    std::str::from_utf8(bytes).map_err(|_| {
        unsupported("language sorting requires valid UTF-8 keys; use LC_COLLATE=C for raw bytes")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories_precedence_and_unknown_collation_are_independent() {
        let policy = Policy::resolve(|key| match key {
            "LANG" => Some("de_DE.UTF-8".into()),
            "LC_NUMERIC" => Some("en_US.utf8".into()),
            "LC_COLLATE" => Some("unknown".into()),
            _ => None,
        });
        assert_eq!(policy.numeric, Profile::EN_NUMERIC);
        assert!(policy.utf8);
        assert!(policy.sorting().is_err());
        let policy = Policy::resolve(|key| match key {
            "LC_ALL" => Some("C.UTF-8".into()),
            "LANG" => Some("unknown".into()),
            _ => None,
        });
        assert_eq!(policy.numeric, Profile::C);
        assert!(policy.utf8);
        assert!(policy.sorting().is_ok());
    }
}
