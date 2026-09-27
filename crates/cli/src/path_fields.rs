//! Byte-field path rules matching GNU datamash path operations.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Directory,
    Base,
    Extension,
    Bare,
}

impl Operation {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Directory => "dirname",
            Self::Base => "basename",
            Self::Extension => "extname",
            Self::Bare => "barename",
        }
    }

    pub(super) fn from_name(name: &[u8]) -> Option<Self> {
        [Self::Directory, Self::Base, Self::Extension, Self::Bare]
            .into_iter()
            .find(|op| name.eq_ignore_ascii_case(op.name().as_bytes()))
    }

    pub(super) fn select(self, input: &[u8]) -> &[u8] {
        // GNU treats a genuinely empty raw field separately from a NUL prefix.
        if input.is_empty() && self != Self::Directory {
            return b"";
        }
        let path = &input[..memchr::memchr(0, input).unwrap_or(input.len())];
        if self == Self::Directory {
            return directory(path);
        }
        let base = basename(path);
        match self {
            Self::Base => base,
            Self::Extension => extension_start(base).map_or(b"", |start| &base[start + 1..]),
            Self::Bare => &base[..extension_start(base).unwrap_or(base.len())],
            Self::Directory => unreachable!("directory handled before basename"),
        }
    }
}

fn without_trailing_slashes(path: &[u8]) -> &[u8] {
    let end = path
        .iter()
        .rposition(|&byte| byte != b'/')
        .map_or(0, |at| at + 1);
    &path[..end]
}

fn directory(path: &[u8]) -> &[u8] {
    if path.is_empty() {
        return b".";
    }
    let trimmed = without_trailing_slashes(path);
    if trimmed.is_empty() {
        return if path.len() == 2 { b"//" } else { b"/" };
    }
    let Some(slash) = memchr::memrchr(b'/', trimmed) else {
        return b".";
    };
    let parent = without_trailing_slashes(&trimmed[..slash]);
    if parent.is_empty() {
        if slash == 1 { b"//" } else { b"/" }
    } else {
        parent
    }
}

fn basename(path: &[u8]) -> &[u8] {
    if path.is_empty() {
        return b".";
    }
    let trimmed = without_trailing_slashes(path);
    if trimmed.is_empty() {
        return b"/";
    }
    let start = memchr::memrchr(b'/', trimmed).map_or(0, |at| at + 1);
    &trimmed[start..]
}

fn extension_start(base: &[u8]) -> Option<usize> {
    let mut end = base.len();
    let mut addon_start = None;
    loop {
        let Some(dot) = base[..end]
            .iter()
            .rposition(|byte| !byte.is_ascii_alphanumeric())
        else {
            return addon_start;
        };
        if dot == 0 || base[dot] != b'.' {
            return addon_start;
        }
        if !matches!(
            &base[dot..end],
            b".gz" | b".xz" | b".lz" | b".gpg" | b".bz2" | b".std"
        ) {
            return Some(dot);
        }
        addon_start = Some(dot);
        end = dot;
    }
}

#[cfg(test)]
mod tests {
    use super::Operation::*;

    #[test]
    fn roots_nul_and_empty_are_distinct() {
        for (input, dir, base) in [
            (b"".as_slice(), b".".as_slice(), b"".as_slice()),
            (b"\0x", b".", b"."),
            (b"//", b"//", b"/"),
            (b"///", b"/", b"/"),
            (b"//a//", b"//", b"a"),
            (b"x//a/../b//", b"x//a/..", b"b"),
            (b"a\0/b", b".", b"a"),
        ] {
            assert_eq!(Directory.select(input), dir);
            assert_eq!(Base.select(input), base);
        }
    }

    #[test]
    fn suffix_stopping_rules_and_long_chains() {
        for (input, bare, ext) in [
            (b"..".as_slice(), b".".as_slice(), b"".as_slice()),
            (b".hidden", b".hidden", b""),
            (b"a..gz", b"a", b".gz"),
            (b"a.x-y.gz", b"a.x-y", b"gz"),
            (b"a.\xff.gz", b"a.\xff", b"gz"),
            (b"a.tar.GZ", b"a.tar", b"GZ"),
            (b"a.123.gz", b"a", b"123.gz"),
        ] {
            assert_eq!(Bare.select(input), bare);
            assert_eq!(Extension.select(input), ext);
        }
        let mut input = b"x.tar".to_vec();
        input.extend_from_slice(&b".gz".repeat(100_000));
        assert_eq!(Bare.select(&input), b"x");
        assert_eq!(Extension.select(&input), &input[2..]);
    }
}
