#[path = "support/oracle.rs"]
mod oracle;
// Independent directed MPFR challenges for the boundary certificate.
#[path = "../src/boundary_exp.rs"]
mod boundary_exp;
use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};
use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended as Extended};
use std::{collections::BTreeSet, env, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    let mut constants = Consts::new().unwrap();
    let mut cases = BTreeSet::new();
    let mut targets = vec![
        // Half minimum subnormal, normal transition, overflow midpoint.
        BigFloat::from_raw_parts(&[0, 1 << 63], 128, Sign::Pos, -16445, false),
        BigFloat::from_raw_parts(&[0, u64::MAX], 128, Sign::Pos, -16382, false),
        BigFloat::from_raw_parts(&[1 << 63, u64::MAX], 128, Sign::Pos, 16384, false),
    ];
    for power in -16445..=-16381 {
        targets.push(BigFloat::from_raw_parts(
            &[1 << 63],
            64,
            Sign::Pos,
            power + 1,
            false,
        ));
    }
    for target in targets {
        let log = target.ln(128, RoundingMode::ToEven, &mut constants);
        assert!(log.err().is_none());
        let (words, _, sign, exponent, _) = log.as_raw_parts().unwrap();
        let text = format!(
            "{}0x{:016x}{:016x}p{}",
            if sign == Sign::Neg { "-" } else { "" },
            words[1],
            words[0],
            exponent - 128
        );
        let center = Extended::from_str_r(&text, Round::NearestTiesToEven)
            .unwrap()
            .value
            .to_bits();
        neighbors(&mut cases, center, 8);
    }
    for text in [
        "-16384", "16384", "-11400", "-11356", "11356", "11357", "-1", "1",
    ] {
        let center = Extended::from_str_r(text, Round::NearestTiesToEven)
            .unwrap()
            .value
            .to_bits();
        neighbors(&mut cases, center, 2);
    }
    let mut random = 0x89d1c0ffeed5eedu64;
    for _ in 0..2048 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let text = format!(
            "{}113{}.{}",
            if random & 1 == 0 { "-" } else { "" },
            random % 10,
            random
        );
        cases.insert(
            Extended::from_str_r(&text, Round::NearestTiesToEven)
                .unwrap()
                .value
                .to_bits(),
        );
    }
    writeln!(output, "input\tcandidate\tmpfr_down\tmpfr_up\tresult")?;
    let mut fallback = 0;
    for bits in &cases {
        let (expected, records) = oracle::directed(&args[1], "exp", *bits)?;
        let exponent = ((bits >> 64) & 0x7fff) as i32;
        let actual = if exponent >= 16397 {
            None
        } else {
            let source = BigFloat::from_raw_parts(
                &[*bits as u64],
                64,
                if bits >> 79 != 0 {
                    Sign::Neg
                } else {
                    Sign::Pos
                },
                exponent - 16382,
                false,
            );
            boundary_exp::exp(&source, &mut constants).map_err(|e| format!("{e:?}"))?
        };
        if let Some(value) = actual {
            if value != expected {
                return Err(
                    format!("mismatch {bits:020x}: {value:020x} vs {:020x}", expected).into(),
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
        "cases={} fallback={fallback} certified_matches={} mismatches=0",
        cases.len(),
        cases.len() - fallback
    );
    Ok(())
}

fn neighbors(cases: &mut BTreeSet<u128>, center: u128, count: usize) {
    let mut lower = Extended::from_bits(center);
    let mut upper = lower;
    cases.insert(center);
    for _ in 0..count {
        lower = lower.next_down().value;
        upper = upper.next_up().value;
        cases.insert(lower.to_bits());
        cases.insert(upper.to_bits());
    }
}
