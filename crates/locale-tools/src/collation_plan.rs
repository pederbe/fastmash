//! Fixed collation word lists and raw output-order comparison.
//!
//! Unicode-dependent alphabet planning and native execution are separate.

use crate::Result;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderDifference {
    /// Complete projected order equality, independently of displacement count.
    pub equal: bool,
    /// Full sorted set outside the original bisect-left reconstruction.
    pub displaced: Vec<String>,
}

/// The historical digit workload, sorted and deduplicated without collation.
pub fn digit_words() -> Vec<String> {
    let mut words = BTreeSet::new();
    for digit in '0'..='9' {
        words.insert(digit.to_string());
        words.insert(format!("a{digit}"));
        words.insert(format!("{digit}a"));
        for other in '0'..='9' {
            words.insert(format!("{digit}{other}"));
        }
    }
    words.extend(["a", "b", "z"].map(str::to_owned));
    words.into_iter().collect()
}

/// Fixed punctuation, symbols, spaces and name-shaped words. Exact strings
/// remain distinct. No case conversion or Unicode normalization is applied.
pub fn punctuation_words() -> Vec<String> {
    let mut symbols: Vec<char> = ('!'..='~')
        .filter(|ch| !ch.is_ascii_alphanumeric())
        .collect();
    symbols.extend(('\u{a1}'..='\u{bf}').filter(|&ch| {
        !matches!(
            ch,
            '\u{aa}' | '\u{ad}' | '\u{b2}' | '\u{b3}' | '\u{b9}' | '\u{ba}'
        )
    }));
    symbols.extend([
        '×', '÷', '‐', '–', '—', '‘', '’', '“', '”', '„', '†', '‡', '•', '…', '‰', '′', '″', '‹',
        '›', '€', '₹', '№', '™', '←', '→', '↑', '↓', '∀', '∂', '∑', '−', '√', '∞', '≈', '≠', '≤',
        '≥', '■', '□', '●', '○', '★', '♀', '♂', '♪',
    ]);
    let mut words = BTreeSet::new();
    for symbol in &symbols {
        words.extend([
            symbol.to_string(),
            format!("{symbol}a"),
            format!("a{symbol}"),
            format!("a{symbol}b"),
            format!("{symbol}1"),
            format!("1{symbol}2"),
            format!("A{symbol}"),
            format!("{symbol}{symbol}"),
            format!("a{symbol}{symbol}b"),
        ]);
    }
    let ascii: Vec<_> = symbols
        .iter()
        .copied()
        .filter(|&symbol| symbol < '\x7f')
        .collect();
    for left in &ascii {
        for right in &ascii {
            words.insert(format!("{left}{right}"));
        }
    }
    words.extend(
        [
            "a b",
            "a  b",
            " a",
            "a ",
            "ab",
            "a b c",
            "a-b c",
            "a b-c",
            "-",
            "+",
            ".",
            "-1",
            "+1",
            "1",
            "-1.5",
            "1e-3",
            "-0",
            "+0",
            "0.5",
            "-.5",
            "1,5",
            "1.5",
            "c.123A>G",
            "c.123A>T",
            "c.12_13del",
            "c.12-1G>A",
            "c.12+1G>A",
            "p.Arg97Gly",
            "p.(Arg97Gly)",
            "p.Arg97*",
            "chr1:123-456",
            "chr1_123",
            "a-1",
            "a_1",
            "a.1",
            "a1",
            "A-1",
            "A_1",
            "b-1",
            "b_1",
            "x.y.z",
            "x-y-z",
            "x_y_z",
            "o'neil",
            "oneil",
            "o-neil",
            "O’Neil",
            "（中",
            "中（",
            "中国（北京）",
            "中国北京（）",
            "-中a_",
            "中-a-",
            "–ča",
            "č–a",
            "-ča_",
            "č-a-",
            "-ła_",
            "ł-a-",
            "’áb",
            "á’b",
            "-ña_",
            "ñ-a-",
            "a\u{a0}-b",
            "a-\u{a0}b",
            "Cia\u{a0}Ltda",
            "Cia.\u{a0}Ltda",
            "ul\u{a0}.Dluga",
            "ul.\u{a0}Dluga",
            "12 A",
            "12-A",
            "12A",
            "1-2A",
            "file1.txt",
            "File1.txt",
            "file(1).txt",
            "c.1-21G>A",
            "c.121G>A",
            "c.12+1G>A",
            "c.12-1G>A",
        ]
        .map(str::to_owned),
    );
    words.into_iter().collect()
}

/// Compare successful command outputs after projecting their first TAB fields.
/// Byte CR/LF/CRLF separate records. Only those fields must be valid UTF-8.
/// Native statuses and diagnostics belong to the caller.
pub fn compare_order_outputs(candidate: &[u8], reference: &[u8]) -> Result<OrderDifference> {
    let candidate = parse_order(candidate)?;
    let reference = parse_order(reference)?;
    let equal = candidate == reference;
    let rank: BTreeMap<_, _> = reference
        .iter()
        .enumerate()
        .map(|(at, &word)| (word, at))
        .collect();
    let mut tails: Vec<usize> = Vec::new();
    let mut tail_at: Vec<&str> = Vec::new();
    let mut parent = BTreeMap::new();
    for &word in &candidate {
        let &position = rank
            .get(word)
            .ok_or_else(|| format!("candidate word has no reference rank: {word:?}"))?;
        let at = tails.partition_point(|&tail| tail < position);
        // Word-keyed predecessors intentionally retain the original behavior
        // when a word occurs more than once.
        parent.insert(word, if at == 0 { None } else { Some(tail_at[at - 1]) });
        if at == tails.len() {
            tails.push(position);
            tail_at.push(word);
        } else {
            tails[at] = position;
            tail_at[at] = word;
        }
    }
    let mut keep = BTreeSet::new();
    let mut word = tail_at.last().copied();
    while let Some(current) = word {
        keep.insert(current);
        word = parent[current];
    }
    let displaced = candidate
        .into_iter()
        .filter(|&word| !keep.contains(word))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(OrderDifference { equal, displaced })
}

fn parse_order(mut output: &[u8]) -> Result<Vec<&str>> {
    let mut words = Vec::new();
    while !output.is_empty() {
        let end = output
            .iter()
            .position(|&byte| matches!(byte, b'\r' | b'\n'));
        let line = &output[..end.unwrap_or(output.len())];
        let field = line.split(|&byte| byte == b'\t').next().unwrap_or_default();
        words.push(std::str::from_utf8(field)?);
        output = match end {
            Some(end) => {
                let crlf = output[end] == b'\r' && output.get(end + 1) == Some(&b'\n');
                &output[end + 1 + usize::from(crlf)..]
            }
            None => &[],
        };
    }
    Ok(words)
}
