//! Explicit C/POSIX text semantics, independent of host locale data.
use std::cmp::Ordering;

pub(super) fn compare(a: &[u8], b: &[u8], ignore_case: bool) -> Ordering {
    if ignore_case {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    } else {
        #[cfg(target_env = "musl")]
        {
            memx::memcmp(a, b)
        }
        #[cfg(not(target_env = "musl"))]
        {
            a.cmp(b)
        }
    }
}

pub(super) fn equal_bytes(a: &[u8], b: &[u8]) -> bool {
    #[cfg(target_env = "musl")]
    {
        memx::memeq(a, b)
    }
    #[cfg(not(target_env = "musl"))]
    {
        a == b
    }
}

pub(super) fn same_group(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // Equal wide keys are common within a group. The bulk comparison avoids
    // scanning them byte by byte; the loop retains embedded-NUL semantics.
    if !ignore_case && a.len() >= 8 && equal_bytes(a, b) {
        return true;
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
    fn wide_group_keys_keep_embedded_nul_and_case_folding_semantics() {
        let a = [b'a'; 128];
        assert!(same_group(&a, &a, false));
        let mut a = a;
        a[64] = 0;
        let mut b = a;
        b[96] = b'z';
        assert!(same_group(&a, &b, false));
        b[63] = b'z';
        assert!(!same_group(&a, &b, false));
        assert!(!same_group(&a, &a[..127], false));
        assert!(same_group(&[b'a'; 128], &[b'A'; 128], true));
    }
    #[test]
    fn grouping_preserves_length_and_c_string_semantics() {
        assert!(same_group(b"a\0x", b"A\0y", true));
        assert!(!same_group(b"a\0x", b"A\0yy", true));
        assert!(!same_group(b"a", b"A", false));
        assert!(!same_group(b"\xc4", b"\xe4", true));
        assert!(compare(b"[", b"Z", true).is_lt());
    }
}
