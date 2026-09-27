#[path = "support/oracle.rs"]
mod oracle;
// Reuses the reviewed external MPFR oracle; production never links to MPFR.
#[path = "../src/guarded_log.rs"]
mod guarded_log;
#[path = "../src/log_table.rs"]
mod log_table;

use std::{collections::BTreeSet, env, fs::OpenOptions, io::Write, process::Command};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = env::args().collect();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[2])?;
    // Both directed endpoints strictly inside this Q112 bin establish floor(ln2*2^112).
    let mut constant = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(std::path::Path::new(&a[2]).with_extension("ln2.tsv"))?;
    for direction in ["down", "up"] {
        let result = Command::new(&a[1])
            .args(["log", "40008000000000000000", "256", direction])
            .output()?;
        if !result.status.success() {
            return Err("ln2 oracle failed".into());
        }
        let text = String::from_utf8(result.stdout)?;
        let fields: Vec<_> = text.trim().split('\t').collect();
        if fields.len() != 7
            || fields[1] != "0"
            || !fields[0].starts_with("b17217f7d1cf79abc9e3b39803f2")
            || fields[0].len() != 65
            || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
            || fields[0][28..].bytes().all(|b| b == b'0')
            || fields[4] != "-1000000"
            || fields[5] != "1000000"
            || !fields[6].starts_with("overflow=0,underflow=0,erange=0,nan=0,")
        {
            return Err("ln2 floor not certified".into());
        }
        writeln!(constant, "{direction}\t{}", text.trim())?;
    }
    let mut cases = BTreeSet::new();
    for e in [
        1u128, 2, 3, 8192, 16318, 16382, 16383, 16384, 16385, 24576, 32765, 32766,
    ] {
        for s in [
            1u128 << 63,
            (1 << 63) + 1,
            (1 << 63) + 2,
            0xb504f333f9de6484,
            0xc000000000000000,
            0xbfffffffffffffff,
            0xc000000000000001,
            u64::MAX as u128 - 1,
            u64::MAX as u128,
        ] {
            cases.insert((e << 64) | s);
        }
    }
    for w in 1..64 {
        for s in [1u128 << (w - 1), (1u128 << w) - 1] {
            cases.insert(s);
        }
    }
    for offset in 1..=128u128 {
        cases.insert((16383u128 << 64) | ((1u128 << 63) + offset));
        cases.insert((16382u128 << 64) | (u64::MAX as u128 - offset + 1));
    }
    let mut random = 0x74d1c0ffeed5eedu64;
    for _ in 0..8192 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let e = 1 + (random % 32766);
        cases.insert((u128::from(e) << 64) | u128::from(random | (1 << 63)));
    }
    writeln!(output, "input\tcandidate\tmpfr_down\tmpfr_up\tresult")?;
    let mut fallback = 0;
    for bits in &cases {
        let (expected, records) = oracle::directed(&a[1], "log", *bits)?;
        let actual = guarded_log::log(*bits);
        if let Some(actual) = actual {
            if actual != expected {
                return Err(
                    format!("mismatch {bits:020x}: {actual:020x} vs {:020x}", expected).into(),
                );
            }
        } else {
            fallback += 1;
        }
        writeln!(
            output,
            "{bits:020x}\t{}\t{}\t{}\t{:020x}",
            actual.map_or("fallback".into(), |v| format!("{v:020x}")),
            records[0],
            records[1],
            expected
        )?;
    }
    println!(
        "cases={} fallback={} certified_matches={} mismatches=0",
        cases.len(),
        fallback,
        cases.len() - fallback
    );
    Ok(())
}
