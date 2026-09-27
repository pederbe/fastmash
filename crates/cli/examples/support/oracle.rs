//! Shared protocol for the reviewed, external directed MPFR oracle.
use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended as Extended};
use std::{error::Error, process::Command};

pub fn directed(
    executable: &str,
    operation: &str,
    bits: u128,
) -> Result<(u128, [String; 2]), Box<dyn Error>> {
    let mut endpoints = [0; 2];
    let mut records = [String::new(), String::new()];
    for (index, direction) in ["down", "up"].into_iter().enumerate() {
        let result = Command::new(executable)
            .args([operation, &format!("{bits:020x}"), "256", direction])
            .output()?;
        if !result.status.success() {
            return Err(format!(
                "oracle {operation} failed for {bits:020x}: {}",
                String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }
        let text = String::from_utf8(result.stdout)?;
        let fields: Vec<_> = text.trim().split('\t').collect();
        if fields.len() != 7
            || fields[4] != "-1000000"
            || fields[5] != "1000000"
            || !fields[6].starts_with("overflow=0,underflow=0,erange=0,nan=0,")
        {
            return Err("oracle metadata mismatch".into());
        }
        let (sign, digits) = fields[0]
            .strip_prefix('-')
            .map_or(("", fields[0]), |s| ("-", s));
        let exponent = fields[1]
            .parse::<i32>()?
            .checked_mul(4)
            .ok_or("oracle exponent overflow")?;
        endpoints[index] = Extended::from_str_r(
            &format!("{sign}0x0.{digits}p{exponent}"),
            Round::NearestTiesToEven,
        )
        .map_err(|_| "oracle value parse failed")?
        .value
        .to_bits();
        records[index] = text.trim().replace('\t', ",");
    }
    if endpoints[0] != endpoints[1] {
        return Err("MPFR rounding not certified".into());
    }
    Ok((endpoints[0], records))
}
