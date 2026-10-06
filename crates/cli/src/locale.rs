//! Invocation-owned, pinned locale semantics; never consult host locale data.
use super::{Failure, collation_glibc, unsupported};
use fastmash_conversion::profile::Profile;
use icu_collator::{
    Collator as IcuCollator, CollatorBorrowed,
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
/// A locale name's rules: whether its character set is UTF-8, its numbers,
/// and its collation; `None` for a glibc locale whose numbers Fastmash
/// refuses. The name is `language[_territory][.codeset][@modifier]`:
///
/// - A name glibc has no locale for behaves as C, as it does in GNU datamash
///   (`setlocale` fails and leaves the C locale).
/// - A modifier glibc has a locale for (`sr_RS@latin`, `de_DE@euro`) has its
///   own numbers, which can differ from its base's; glibc drops any other.
/// - A character set other than UTF-8 keeps the locale's numbers when their
///   separators are ASCII, the same bytes in any such set; sorting in it is
///   refused.
fn recognized(bytes: &[u8]) -> Option<(bool, Profile, Collation)> {
    let (name, modifier) = match bytes.iter().position(|&b| b == b'@') {
        Some(at) => (&bytes[..at], Some(&bytes[at..])),
        None => (bytes, None),
    };
    let (base, codeset) = match name.iter().position(|&b| b == b'.') {
        Some(dot) => (&name[..dot], Some(&name[dot + 1..])),
        None => (name, None),
    };
    let utf8 = match codeset {
        Some(codeset) => {
            codeset.eq_ignore_ascii_case(b"UTF-8") || codeset.eq_ignore_ascii_case(b"utf8")
        }
        // glibc locales whose default character set is UTF-8, such as hi_IN.
        None => super::numeric_locales::BARE_UTF8
            .binary_search(&base)
            .is_ok(),
    };
    if let b"C" | b"POSIX" = base {
        return Some((utf8, Profile::C, Collation::Bytes));
    }
    let numbers = |key: &[u8]| {
        let table = super::numeric_locales::NUMERIC;
        table
            .binary_search_by(|(entry, _)| (*entry).cmp(key))
            .ok()
            .map(|at| table[at].1)
    };
    let refused = super::numeric_locales::REFUSED.binary_search(&base).is_ok();
    // A modifier locale of its own, else the base (glibc drops the modifier).
    let own = modifier.and_then(|modifier| {
        let mut key = base.to_vec();
        key.extend_from_slice(modifier);
        numbers(&key)
    });
    let numeric = match own.or_else(|| numbers(base)) {
        Some(numeric) => numeric,
        None if refused => return None,
        None => return Some((false, Profile::C, Collation::Bytes)),
    };
    if !utf8 && !(numeric.radix().is_ascii() && numeric.thousands().is_ascii()) {
        return None;
    }
    // Locales whose letter order the Unicode collator reproduces, in UTF-8
    // and without a modifier of their own.
    let table = super::collation_locales::COLLATION;
    let collation = if utf8 && own.is_none() {
        table
            .binary_search_by(|(entry, _)| (*entry).cmp(base))
            .map_or(Collation::Unsupported, |at| {
                Collation::Language(table[at].1)
            })
    } else {
        Collation::Unsupported
    };
    Some((utf8, numeric, collation))
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
    pub(super) fn resolve(get: impl Fn(&str) -> Option<OsString>) -> Self {
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
    pub fn collator(self) -> Result<Option<Collator>, Failure> {
        self.sorting()?;
        let Collation::Language(tag) = self.collation else {
            return Ok(None);
        };
        Collator::new(tag).map(Some)
    }
}

/// How glibc weighs a character where the Unicode collator weighs it
/// differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    /// Ignored at the first three levels and compared at the fourth by its
    /// rank: its place among the characters glibc ignores, after those the
    /// locale weighs first.
    Ignored(u16),
    /// Weighed before every letter and digit at the first level, and at the
    /// fourth ranked below every ignored character, by its place among the
    /// locale's.
    First(u16),
    /// A letter or digit without a fourth-level weight.
    Unweighted,
    /// Any other character: the Unicode collator orders it, and at the
    /// fourth level it follows every ignored character. The collator reads
    /// it as its canonical decomposition, this many characters long.
    Other(u8),
}

/// The fourth level of a character the Unicode collator orders, as the
/// collator reads it: one per character of its canonical decomposition.
const LETTER: Weight = Weight::Other(1);

/// Language collation in the order GNU sort gets from glibc: the Unicode
/// collator orders the letters and digits (and whatever else glibc weighs),
/// and glibc's own tables (`collation_glibc`) give the characters it ignores
/// at the first three levels and compares at the fourth, by their order in
/// its iso14651_t1_common.
pub(super) struct Collator {
    icu: CollatorBorrowed<'static>,
    /// The ignored characters this locale weighs first instead.
    first: &'static [u32],
    /// This locale's letters without a fourth-level weight, beyond those
    /// every locale has.
    unweighted: &'static [(u32, u32)],

    ascii: [Weight; 128],
    /// The Basic Multilingual Plane's weights, by block of 256 characters,
    /// each classified when a key first uses it.
    blocks: Box<[std::sync::OnceLock<Block>]>,
}

/// The weights of a block of 256 characters.
enum Block {
    /// Every character of the block weighs this.
    Uniform(Weight),
    /// By character.
    Mixed(Box<[Weight]>),
    /// Not kept, for want of memory: classified each time.
    Unknown,
}

/// The room the scratch of [`Collator::write_key`] keeps between keys; a
/// longer key's is given back.
const SCRATCH_ROOM: usize = 1 << 16;

thread_local! {
    /// The text the Unicode collator reads, and the fourth level, reused
    /// between keys.
    static SCRATCH: std::cell::RefCell<(String, Vec<u8>)> =
        const { std::cell::RefCell::new((String::new(), Vec::new())) };
}

#[cfg(test)]
thread_local! {
    /// Writes a sort key may make before its buffer fails to grow.
    pub(super) static KEY_WRITES_BEFORE_FAILURE: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// A sort key's buffer could not grow.
pub(super) struct KeyTooLarge;

/// A sort key buffer that reports when it cannot grow, so that a key too
/// large for memory refuses instead of aborting.
struct KeyBuffer<'a>(&'a mut Vec<u8>);

impl KeyBuffer<'_> {
    /// Makes room for `additional` more bytes. The room reserved for the key
    /// usually suffices, so growing is out of line.
    #[inline]
    fn room(&mut self, additional: usize) -> Result<(), KeyTooLarge> {
        #[cfg(test)]
        if KEY_WRITES_BEFORE_FAILURE.with(|count| match count.get() {
            Some(0) => true,
            Some(n) => {
                count.set(Some(n - 1));
                false
            }
            None => false,
        }) {
            return Err(KeyTooLarge);
        }
        if self.0.capacity() - self.0.len() < additional {
            self.grow(additional)?;
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self, additional: usize) -> Result<(), KeyTooLarge> {
        self.0.try_reserve(additional).map_err(|_| KeyTooLarge)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), KeyTooLarge> {
        self.room(bytes.len())?;
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    /// Appends a number, encoded so that byte order is the numbers' order:
    /// two bytes below 0xff00, else 0xff and eight bytes.
    fn number(&mut self, number: u64) -> Result<(), KeyTooLarge> {
        match u16::try_from(number) {
            Ok(short) if short < 0xff00 => self.bytes(&short.to_be_bytes()),
            _ => {
                self.bytes(&[0xff])?;
                self.bytes(&number.to_be_bytes())
            }
        }
    }
}

/// Where a count of letters is written after this, the next item ranks above
/// a letter: its number is `AFTER_SKIPS - count`, above every count.
const AFTER_SKIPS: u64 = 1 << 62;

/// The weight a letter has in [`Fourth`], above every rank.
const LETTER_RANK: u16 = u16::MAX;

/// glibc's fourth level of a key, written as it is compared. glibc compares
/// it item by item: an ignored character by its rank, or a weighted
/// character, which ranks above every ignored one (weighted characters of
/// the key's letters that tie at the first three levels tie here too).
/// Before an item's weight it compares the letters without a fourth-level
/// weight skipped since the previous item (its `position` rule: more ranks
/// higher), and a key that runs out of items first ranks lower.
///
/// Runs of weighted characters with none skipped are written as counts. A
/// count is followed by the item that ends the run: an ignored character
/// with none skipped (its rank), or the key's end (nothing), both below a
/// weighted character, so a shorter run ranks lower; or an item after
/// skipped letters, above a weighted character, so a shorter run ranks
/// higher: its number is then `AFTER_SKIPS - count`, and the skipped count
/// plus one and its weight follow.
struct Fourth<'a> {
    out: KeyBuffer<'a>,
    letters: u64,
    skipped: u64,
}

impl Fourth<'_> {
    /// `count` weighted characters.
    fn letters(&mut self, count: u64) -> Result<(), KeyTooLarge> {
        if count != 0 && self.skipped != 0 {
            self.after_skips(LETTER_RANK)?;
            self.letters = count - 1;
        } else {
            self.letters += count;
        }
        Ok(())
    }

    /// A letter without a fourth-level weight.
    fn skip(&mut self) {
        self.skipped += 1;
    }

    /// An ignored character of rank `rank`.
    fn ignored(&mut self, rank: u16) -> Result<(), KeyTooLarge> {
        if self.skipped == 0 {
            self.out.number(self.letters)?;
            self.out.bytes(&rank.to_be_bytes())?;
        } else {
            self.after_skips(rank)?;
        }
        self.letters = 0;
        Ok(())
    }

    fn after_skips(&mut self, weight: u16) -> Result<(), KeyTooLarge> {
        self.out
            .number(AFTER_SKIPS - self.letters.min(AFTER_SKIPS - 1))?;
        self.out.number(self.skipped + 1)?;
        self.out.bytes(&weight.to_be_bytes())?;
        self.skipped = 0;
        Ok(())
    }

    /// The key's end: letters skipped after the last item do not count.
    fn end(&mut self) -> Result<(), KeyTooLarge> {
        self.out.number(self.letters)
    }
}

impl icu_collator::CollationKeySink for KeyBuffer<'_> {
    type Error = KeyTooLarge;
    type State = ();
    type Output = ();
    #[inline]
    fn write(&mut self, _: &mut (), bytes: &[u8]) -> Result<(), KeyTooLarge> {
        self.bytes(bytes)
    }
    /// The collator writes most of a key a byte at a time.
    #[inline]
    fn write_byte(&mut self, _: &mut (), byte: u8) -> Result<(), KeyTooLarge> {
        self.room(1)?;
        self.0.push(byte);
        Ok(())
    }
    fn finish(&mut self, _: ()) -> Result<(), KeyTooLarge> {
        Ok(())
    }
}

impl Collator {
    fn new(tag: &'static str) -> Result<Self, Failure> {
        let language = Locale::try_from_str(tag).expect("collation tags are valid BCP 47");
        let mut options = CollatorOptions::default();
        // The characters glibc ignores never reach the collator, which
        // weighs everything else.
        options.alternate_handling = Some(AlternateHandling::NonIgnorable);
        options.strength = Some(Strength::Tertiary);
        let icu = IcuCollator::try_new(language.into(), options)
            .map_err(|_| unsupported("compiled locale data unavailable"))?;
        let (first, unweighted) = collation_glibc::LOCALES
            .binary_search_by(|(entry, ..)| (*entry).cmp(tag))
            .map_or((&[][..], &[][..]), |at| {
                let (_, first, unweighted) = collation_glibc::LOCALES[at];
                (first, unweighted)
            });
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(256)
            .map_err(|_| unsupported("command memory allocation failed"))?;
        blocks.resize_with(256, std::sync::OnceLock::new);
        let mut collator = Self {
            icu,
            first,
            unweighted,
            ascii: [LETTER; 128],
            blocks: blocks.into_boxed_slice(),
        };
        for byte in 0..128u8 {
            collator.ascii[usize::from(byte)] = collator.classify(char::from(byte));
        }
        Ok(collator)
    }

    #[inline]
    fn weight(&self, character: char) -> Weight {
        let point = u32::from(character);
        if let Some(&weight) = self.ascii.get(point as usize) {
            return weight;
        }
        let Some(block) = self.blocks.get((point >> 8) as usize) else {
            return self.classify(character);
        };
        match block.get_or_init(|| self.block(point >> 8)) {
            Block::Uniform(weight) => *weight,
            Block::Mixed(weights) => weights[(point & 0xff) as usize],
            Block::Unknown => self.classify(character),
        }
    }

    /// The weights of the 256 characters from `block * 256`.
    #[cold]
    fn block(&self, block: u32) -> Block {
        let weight = |at: u32| char::from_u32(block << 8 | at).map_or(LETTER, |c| self.classify(c));
        let uniform = weight(0);
        if (1..256).all(|at| weight(at) == uniform) {
            return Block::Uniform(uniform);
        }
        let mut weights = Vec::new();
        if weights.try_reserve_exact(256).is_err() {
            return Block::Unknown;
        }
        weights.extend((0..256).map(weight));
        Block::Mixed(weights.into_boxed_slice())
    }

    fn classify(&self, character: char) -> Weight {
        let point = u32::from(character);
        let ignored = collation_glibc::IGNORED;
        let at = ignored.partition_point(|&(first, ..)| first <= point);
        if let Some(&(first, last, rank)) = at.checked_sub(1).map(|at| &ignored[at])
            && point <= last
        {
            // The characters a locale weighs first rank below the others, in
            // the table's order; the table's ranks are below 2^16 less those,
            // and consecutive within a range.
            return match self.first.iter().position(|&weighed| weighed == point) {
                Some(at) => Weight::First(at as u16),
                None => Weight::Ignored(rank + (point - first) as u16 + self.first.len() as u16),
            };
        }
        if within(collation_glibc::NO_FOURTH, point) || within(self.unweighted, point) {
            return Weight::Unweighted;
        }
        let decomposed = collation_glibc::DECOMPOSED;
        let at = decomposed.partition_point(|&(first, ..)| first <= point);
        match at.checked_sub(1).map(|at| decomposed[at]) {
            Some((_, last, length)) if point <= last => Weight::Other(length),
            _ => LETTER,
        }
    }

    /// Appends the sort key of `text` to `out`, refusing if `out` cannot
    /// grow: the Unicode collator's key (three levels) of `text` with the
    /// characters glibc ignores as U+0000, which it ignores too, and those
    /// glibc weighs first as U+FFFE, which it weighs below everything; then,
    /// after a byte below every key byte, glibc's fourth level ([`Fourth`]).
    /// Weighted characters count as the collator reads them, decomposed, so
    /// that text it ties with another spelling of it still ties at the fourth
    /// level, and the caller's tie-break by the text's bytes decides, as
    /// glibc does between a decomposed and a precomposed letter.
    pub(super) fn write_key(&self, text: &str, out: &mut Vec<u8>) -> Result<(), KeyTooLarge> {
        let room = text.len().saturating_mul(3).saturating_add(16);
        out.try_reserve(room).map_err(|_| KeyTooLarge)?;
        let mut key = KeyBuffer(out);
        let bytes = text.as_bytes();
        // ASCII letters and digits reach the collator as they are.
        let plain = |byte: u8| byte < 0x80 && self.ascii[usize::from(byte)] == LETTER;
        let Some(start) = bytes.iter().position(|&byte| !plain(byte)) else {
            self.icu.write_sort_key_to(text, &mut key)?;
            key.bytes(&[1])?;
            return key.number(text.len() as u64);
        };
        SCRATCH.with_borrow_mut(|(mapped, fourth)| {
            mapped.clear();
            fourth.clear();
            let written = self.map(text, start, mapped, fourth);
            let result = written.and_then(|()| {
                self.icu.write_sort_key_to(mapped.as_str(), &mut key)?;
                key.bytes(&[1])?;
                key.bytes(fourth)
            });
            // The scratch keeps the room of ordinary keys only.
            if mapped.capacity() > SCRATCH_ROOM {
                *mapped = String::new();
            }
            if fourth.capacity() > SCRATCH_ROOM {
                *fourth = Vec::new();
            }
            result
        })
    }

    /// Appends `text` to `out` as a key's identity, which orders the spellings
    /// the collator ties, if it holds a letter that glibc places after its
    /// canonical decomposition though coded below it
    /// (`collation_glibc::REORDERED`, such as `Ё`): each is written as its
    /// base letter, 0xFF (which no key's UTF-8 holds) and the letter, so that
    /// byte order puts it after its decomposed spelling, as code point order
    /// does for the others. Returns whether it appended; `out` is unchanged
    /// otherwise.
    pub(super) fn write_identity(
        &self,
        text: &str,
        out: &mut Vec<u8>,
    ) -> Result<bool, KeyTooLarge> {
        static PLACED: std::sync::OnceLock<CharSet> = std::sync::OnceLock::new();
        let table = collation_glibc::REORDERED;
        let placed = PLACED.get_or_init(|| CharSet::new(table.iter().map(|&(letter, _)| letter)));
        if text.is_ascii() || !text.chars().any(|c| placed.contains(c)) {
            return Ok(false);
        }
        let mut key = KeyBuffer(out);
        for character in text.chars() {
            let point = u32::from(character);
            if placed.contains(character)
                && let Ok(at) = table.binary_search_by_key(&point, |&(letter, _)| letter)
            {
                let base = char::from_u32(table[at].1).expect("bases are characters");
                key.bytes(base.encode_utf8(&mut [0; 4]).as_bytes())?;
                key.bytes(&[0xff])?;
            }
            key.bytes(character.encode_utf8(&mut [0; 4]).as_bytes())?;
        }
        Ok(true)
    }

    /// Writes `text`, whose first `start` bytes are ASCII letters and digits,
    /// as the Unicode collator reads it to `mapped`, and its fourth level
    /// ([`Fourth`]) to `fourth`.
    fn map(
        &self,
        text: &str,
        start: usize,
        mapped: &mut String,
        fourth: &mut Vec<u8>,
    ) -> Result<(), KeyTooLarge> {
        let bytes = text.as_bytes();
        let plain = |byte: u8| byte < 0x80 && self.ascii[usize::from(byte)] == LETTER;
        let mut fourth = Fourth {
            out: KeyBuffer(fourth),
            letters: start as u64,
            skipped: 0,
        };
        // The text up to `at` is weighed; from `run` it is still to be copied
        // as it is.
        let (mut run, mut at) = (0, start);
        while let Some(character) = text[at..].chars().next() {
            let end = at + character.len_utf8();
            let replacement = match self.weight(character) {
                LETTER if character.is_ascii() => {
                    let letters = bytes[end..].iter().position(|&byte| !plain(byte));
                    let end = letters.map_or(bytes.len(), |letters| end + letters);
                    fourth.letters((end - at) as u64)?;
                    at = end;
                    continue;
                }
                Weight::Other(length) => {
                    fourth.letters(u64::from(length))?;
                    at = end;
                    continue;
                }
                Weight::Unweighted => {
                    fourth.skip();
                    at = end;
                    continue;
                }
                Weight::Ignored(rank) => {
                    fourth.ignored(rank)?;
                    '\0'
                }
                Weight::First(rank) => {
                    fourth.ignored(rank)?;
                    '\u{fffe}'
                }
            };
            let copy = &text[run..at];
            mapped
                .try_reserve(copy.len() + replacement.len_utf8())
                .map_err(|_| KeyTooLarge)?;
            mapped.push_str(copy);
            mapped.push(replacement);
            (run, at) = (end, end);
        }
        let copy = &text[run..];
        mapped.try_reserve(copy.len()).map_err(|_| KeyTooLarge)?;
        mapped.push_str(copy);
        fourth.end()
    }
}

/// Writes `text` to `out` with each sequence that spells a letter in parts
/// in glibc's table (`collation_glibc::CONTRACTIONS`, such as `и` and a
/// combining breve for `й`) replaced by that letter, matching the longest
/// sequence at each position as glibc does. glibc ties each with its letter
/// at every level, where the Unicode collator may not (`L` and a middle dot
/// is only compatibly `Ŀ`). Returns whether anything was replaced; `out` is
/// written only then.
pub(super) fn compose(text: &str, out: &mut String) -> Result<bool, KeyTooLarge> {
    // Every sequence ends in a character outside ASCII.
    if text.is_ascii() {
        return Ok(false);
    }
    let table = collation_glibc::CONTRACTIONS;
    // Text without a sequence's last character has none of the sequences.
    static LAST: std::sync::OnceLock<CharSet> = std::sync::OnceLock::new();
    let last = LAST.get_or_init(|| {
        CharSet::new(
            table
                .iter()
                .filter_map(|(parts, _)| parts.chars().last())
                .map(u32::from),
        )
    });
    if !text.chars().any(|character| last.contains(character)) {
        return Ok(false);
    }
    let (mut composed, mut run, mut at) = (false, 0, 0);
    while let Some(character) = text[at..].chars().next() {
        let first = table.partition_point(|(parts, _)| parts.chars().next() < Some(character));
        let longest = table[first..]
            .iter()
            .take_while(|(parts, _)| parts.starts_with(character))
            .filter(|(parts, _)| text[at..].starts_with(parts))
            .max_by_key(|(parts, _)| parts.len());
        let Some(&(parts, letter)) = longest else {
            at += character.len_utf8();
            continue;
        };
        if !composed {
            out.clear();
            composed = true;
        }
        let copy = &text[run..at];
        out.try_reserve(copy.len() + letter.len_utf8())
            .map_err(|_| KeyTooLarge)?;
        out.push_str(copy);
        out.push(letter);
        at += parts.len();
        run = at;
    }
    if composed {
        let copy = &text[run..];
        out.try_reserve(copy.len()).map_err(|_| KeyTooLarge)?;
        out.push_str(copy);
    }
    Ok(composed)
}

/// A set of characters, tested in constant time: a bitmap for each block of
/// 256 characters that holds any of them.
struct CharSet {
    /// Per block from U+0000, one more than its bitmap's index, or zero.
    blocks: Vec<u16>,
    bitmaps: Vec<[u64; 4]>,
}

impl CharSet {
    fn new(points: impl IntoIterator<Item = u32>) -> Self {
        let mut set = Self {
            blocks: Vec::new(),
            bitmaps: Vec::new(),
        };
        for point in points {
            let block = (point >> 8) as usize;
            if set.blocks.len() <= block {
                set.blocks.resize(block + 1, 0);
            }
            if set.blocks[block] == 0 {
                set.bitmaps.push([0; 4]);
                set.blocks[block] =
                    u16::try_from(set.bitmaps.len()).expect("few blocks hold such characters");
            }
            let bitmap = &mut set.bitmaps[usize::from(set.blocks[block]) - 1];
            bitmap[(point as usize >> 6) & 3] |= 1 << (point & 63);
        }
        set
    }

    #[inline]
    fn contains(&self, character: char) -> bool {
        let point = u32::from(character);
        match self.blocks.get((point >> 8) as usize) {
            Some(&at) if at != 0 => {
                self.bitmaps[usize::from(at) - 1][(point as usize >> 6) & 3] >> (point & 63) & 1
                    != 0
            }
            _ => false,
        }
    }
}

/// Whether `point` is in one of the sorted `ranges`.
fn within(ranges: &[(u32, u32)], point: u32) -> bool {
    let at = ranges.partition_point(|&(first, _)| first <= point);
    at.checked_sub(1).is_some_and(|at| point <= ranges[at].1)
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
        // A collation name glibc has no locale for sorts as C does.
        assert!(policy.collation == Collation::Bytes);
        let policy = Policy::resolve(|key| match key {
            "LANG" => Some("de_DE.UTF-8".into()),
            "LC_COLLATE" => Some("cs_CZ.UTF-8".into()),
            _ => None,
        });
        // A glibc locale whose order Fastmash has not verified is refused.
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

    fn all(name: &'static str) -> Policy {
        Policy::resolve(|key| (key == "LC_ALL").then(|| name.into()))
    }

    fn collator(tag: &'static str) -> Collator {
        let policy = Policy {
            collation: Collation::Language(tag),
            ..Policy::default()
        };
        policy.collator().ok().unwrap().unwrap()
    }

    fn sorted(collator: &Collator, words: &[&'static str]) -> Vec<&'static str> {
        let mut keyed: Vec<(Vec<u8>, &str)> = words
            .iter()
            .map(|&word| {
                let mut key = Vec::new();
                assert!(collator.write_key(word, &mut key).is_ok());
                (key, word)
            })
            .collect();
        keyed.sort();
        keyed.into_iter().map(|(_, word)| word).collect()
    }

    /// Two texts in the documented model's order, one character at a time:
    /// the Unicode collator's order of the texts it reads, then glibc's
    /// fourth level, item by item (letters skipped before an item, then its
    /// weight), where a text that runs out of items first ranks lower.
    fn model_order(collator: &Collator, a: &str, b: &str) -> std::cmp::Ordering {
        let read = |text: &str| {
            let (mut mapped, mut items, mut skipped) = (String::new(), Vec::new(), 0u64);
            for character in text.chars() {
                let (replacement, weight, length) = match collator.classify(character) {
                    Weight::Other(length) => (character, LETTER_RANK, length),
                    Weight::Unweighted => {
                        mapped.push(character);
                        skipped += 1;
                        continue;
                    }
                    Weight::Ignored(rank) => ('\0', rank, 1),
                    Weight::First(rank) => ('\u{fffe}', rank, 1),
                };
                mapped.push(replacement);
                items.push((skipped + 1, weight));
                items.extend((1..length).map(|_| (1, LETTER_RANK)));
                skipped = 0;
            }
            let mut key = Vec::new();
            assert!(collator.icu.write_sort_key_to(&mapped, &mut key).is_ok());
            (key, items)
        };
        read(a).cmp(&read(b))
    }

    /// glibc's contractions become the letters they spell, the longest at
    /// each position, between other text and one after another; other text
    /// is left alone.
    #[test]
    fn contractions_compose_as_glibc_reads_them() {
        let cases = [
            ("и\u{306}", Some("й")),
            ("ди\u{306}к", Some("дйк")),
            ("L\u{b7}l\u{b7}", Some("Ŀŀ")),
            ("L\u{387}", Some("Ŀ")),
            ("\u{cc6}\u{cc2}\u{cd5}", Some("\u{ccb}")),
            ("\u{cc6}\u{cc2}x", Some("\u{cca}x")),
            (
                "\u{fb2}\u{f71}\u{f80}\u{f71}\u{f72}",
                Some("\u{f77}\u{f73}"),
            ),
            ("й", None),
            ("и \u{306}", None),
            ("\u{306}и", None),
            ("e\u{301}", None),
            ("L", None),
            ("", None),
        ];
        for (text, expected) in cases {
            let mut out = String::from("stale");
            let composed = compose(text, &mut out).ok().unwrap();
            assert_eq!(composed.then_some(out.as_str()), expected, "{text:?}");
        }
        // Every table entry composes alone and is sorted for the search.
        let table = collation_glibc::CONTRACTIONS;
        assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for &(parts, letter) in table {
            let mut out = String::new();
            assert!(compose(parts, &mut out).ok().unwrap());
            assert_eq!(out, letter.to_string());
        }
    }

    /// Identities order a letter after its decomposed spelling, as glibc
    /// does: by code point (`é`, and `ǖ`, which glibc places first only in
    /// some contexts), and for the letters coded below their base (`Ё`, `ά`)
    /// by the table, wherever they are in the text.
    #[test]
    fn identities_place_letters_after_their_decomposition() {
        let collator = Collator::new("en-US").ok().unwrap();
        let identity = |text: &str| {
            let mut out = Vec::new();
            if !collator.write_identity(text, &mut out).ok().unwrap() {
                out = text.as_bytes().to_vec();
            }
            out
        };
        let cases = [
            ("e\u{301}", "\u{e9}"),
            ("u\u{308}\u{304}", "\u{1d6}"),
            ("\u{415}\u{308}", "\u{401}"),
            ("x\u{415}\u{308}y", "x\u{401}y"),
            ("\u{3b1}\u{301}", "\u{3ac}"),
            ("\u{415}\u{308}\u{401}", "\u{401}\u{415}\u{308}"),
        ];
        for (before, after) in cases {
            assert!(identity(before) < identity(after), "{before:?} {after:?}");
        }
        let mut out = Vec::new();
        assert!(
            !collator
                .write_identity("\u{e9}t\u{e9}", &mut out)
                .ok()
                .unwrap()
        );
        assert!(out.is_empty());
        // Every listed letter is written after its own base.
        for &(letter, base) in collation_glibc::REORDERED {
            let letter = char::from_u32(letter).unwrap().to_string();
            let mut out = Vec::new();
            assert!(collator.write_identity(&letter, &mut out).ok().unwrap());
            let mut expected = char::from_u32(base).unwrap().to_string().into_bytes();
            expected.push(0xff);
            expected.extend_from_slice(letter.as_bytes());
            assert_eq!(out, expected);
        }
    }

    /// Keys built in runs, with weights kept by block, order texts as the
    /// model does: every text of up to three characters from a set of
    /// letters, letters without a fourth-level weight, characters weighed
    /// first and ignored characters.
    #[test]
    fn language_keys_follow_the_model() {
        let set = [
            "a", "b", "č", "ł", "中", "á", " ", "\u{a0}", "-", "_", "（", "é", "e\u{301}", "1",
        ];
        let mut words = vec![String::new()];
        for length in 1..=3 {
            let shorter: Vec<String> = words
                .iter()
                .filter(|w| w.chars().count() < length)
                .cloned()
                .collect();
            for word in shorter {
                words.extend(set.iter().map(|piece| format!("{word}{piece}")));
            }
        }
        words.sort();
        words.dedup();
        for tag in ["en-US", "es-ES", "hr-HR", "pl-PL", "is-IS", "ja-JP"] {
            let collator = collator(tag);
            for point in 0..=0xffffu32 {
                if let Some(character) = char::from_u32(point) {
                    assert_eq!(collator.weight(character), collator.classify(character));
                }
            }
            let key = |text: &str| {
                let mut key = Vec::new();
                assert!(collator.write_key(text, &mut key).is_ok());
                key
            };
            let mut by_key = words.clone();
            by_key.sort_by_cached_key(|word| key(word));
            let mut by_model = words.clone();
            by_model.sort_by(|a, b| model_order(&collator, a, b));
            assert_eq!(by_key, by_model, "{tag}");
        }
    }

    /// Letters without a fourth-level weight count before the item they
    /// precede, and the spaces es, gl and pl weigh first rank below every
    /// ignored character, as GNU sort 9.7 orders them under glibc 2.43.
    #[test]
    fn language_keys_order_around_unweighted_letters_as_glibc_does() {
        for (tag, glibc) in [
            (
                "en-US",
                &[
                    "（中",
                    "中（",
                    "-中a_",
                    "中-a-",
                    "中国（北京）",
                    "中国北京（）",
                ][..],
            ),
            ("hr-HR", &["-ča_", "–ča", "č-a-", "č–a"]),
            (
                "pl-PL",
                &["-ła_", "ł-a-", "ul\u{a0}.Dluga", "ul.\u{a0}Dluga"],
            ),
            ("is-IS", &["’áb", "á’b"]),
            ("es-MX", &["Cia\u{a0}Ltda", "Cia.\u{a0}Ltda"]),
            (
                "es-ES",
                &[
                    "a \u{ad}b",
                    "a \u{200b}b",
                    "a\u{a0}-b",
                    "a\u{ad} b",
                    "a\u{200b} b",
                    "a-\u{a0}b",
                ],
            ),
            ("gl-ES", &["a\u{a0}-b", "a-\u{a0}b"]),
        ] {
            let mut words = glibc.to_vec();
            words.reverse();
            assert_eq!(sorted(&collator(tag), &words), glibc, "{tag}");
        }
    }

    /// Characters glibc ignores decide only after the letters, in their own
    /// order; currency symbols are weighed, and the Spanish and Polish
    /// locales weigh the space first. The expected orders are GNU sort 9.7's
    /// under glibc 2.43.
    #[test]
    fn language_keys_order_ignored_characters_as_glibc_does() {
        let words = [
            "ab", "a-b", "a+c", "a b", "a0", "a$b", "ac", "Ab", "a_b", "中", "中-", "čb", "c-b",
        ];
        let glibc = [
            "a$b", "a0", "a b", "a-b", "a_b", "ab", "Ab", "a+c", "ac", "c-b", "čb", "中", "中-",
        ];
        assert_eq!(sorted(&collator("en-US"), &words), glibc);
        assert_eq!(sorted(&collator("hr-HR"), &words), glibc);
        let space_first = [
            "a b", "a$b", "a0", "a-b", "a_b", "ab", "Ab", "a+c", "ac", "c-b", "čb", "中", "中-",
        ];
        assert_eq!(sorted(&collator("es-ES"), &words), space_first);
        assert_eq!(sorted(&collator("pl-PL"), &words), space_first);
    }

    #[test]
    fn unknown_names_behave_as_c_as_in_gnu() {
        for name in [
            "xx_YY.UTF-8",
            "unknown",
            "de",
            "english",
            "en_XX.UTF-8@euro",
        ] {
            let policy = all(name);
            assert!(policy.numbers().is_ok(), "{name}");
            assert_eq!(policy.numeric, Profile::C, "{name}");
            assert!(policy.collation == Collation::Bytes, "{name}");
            assert!(!policy.utf8, "{name}");
        }
        let c = all("C.UTF-8@anything");
        assert!(c.utf8 && c.collation == Collation::Bytes && c.numeric == Profile::C);
    }

    #[test]
    fn variants_of_a_known_locale_keep_its_numbers() {
        let de = all("de_DE.UTF-8");
        assert_eq!(de.numeric.radix(), b',');
        // Other character sets: the same ASCII separators, no sorting.
        for name in ["de_DE", "de_DE.ISO-8859-1", "de_DE.iso88591", "de_DE@euro"] {
            let policy = all(name);
            assert!(policy.numbers().is_ok(), "{name}");
            assert_eq!(policy.numeric, de.numeric, "{name}");
            assert!(!policy.utf8, "{name}");
            assert!(policy.sorting().is_err(), "{name}");
        }
        // A separator that is not ASCII has other bytes outside UTF-8.
        let fr = all("fr_FR.UTF-8");
        assert!(!fr.numeric.thousands().is_ascii());
        assert!(all("fr_FR.ISO-8859-1").numbers().is_err());
        assert!(all("fr_FR@euro").numbers().is_err());
        // A modifier locale of glibc's own, with numbers of its own.
        let latin = all("nan_TW.UTF-8@latin");
        assert!(latin.utf8 && latin.numbers().is_ok());
        assert_eq!(latin.numeric.grouping(), b"\x03");
        assert_eq!(all("nan_TW.UTF-8").numeric.grouping(), b"\x04");
        assert!(latin.sorting().is_err());
        assert_eq!(all("de_DE.UTF-8@euro").numeric, de.numeric);
        // glibc drops a modifier it has no locale for.
        let en = all("en_US.UTF-8@nothing");
        assert_eq!(en.numeric, all("en_US.UTF-8").numeric);
        assert!(en.utf8 && en.language());
        // Known locales whose decimal point is not one byte stay refused.
        for name in ["ps_AF.UTF-8", "ps_AF", "ps_AF.UTF-8@nothing"] {
            assert!(all(name).numbers().is_err(), "{name}");
        }
    }
}
