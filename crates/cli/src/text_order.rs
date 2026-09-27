//! Explicit C/POSIX text semantics, independent of host locale data.
use std::cmp::Ordering;

pub(super) fn compare(a: &[u8], b: &[u8], ignore_case: bool) -> Ordering {
    if ignore_case {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    } else {
        a.cmp(b)
    }
}

pub(super) fn same_group(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (&a, &b) in a.iter().zip(b) {
        if if ignore_case {
            !a.eq_ignore_ascii_case(&b)
        } else {
            a != b
        } {
            return false;
        }
        if a == 0 {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouping_preserves_length_and_c_string_semantics() {
        assert!(same_group(b"a\0x", b"A\0y", true));
        assert!(!same_group(b"a\0x", b"A\0yy", true));
        assert!(!same_group(b"a", b"A", false));
        assert!(!same_group(b"\xc4", b"\xe4", true));
        assert!(compare(b"[", b"Z", true).is_lt());
    }
}
