//! Directed range-boundary certificate.
use astro_float_num::{BigFloat, Consts, Error, RoundingMode, Sign};
use rustc_apfloat::{
    Float, FloatConvert, Round,
    ieee::{Quad, X87DoubleExtended as Extended},
};

pub fn exp(source: &BigFloat, constants: &mut Consts) -> Result<Option<u128>, Error> {
    if let Some(error) = source.err() {
        return Err(error);
    }
    let lower = source.exp(128, RoundingMode::Down, constants);
    let upper = source.exp(128, RoundingMode::Up, constants);
    certificate(&lower, &upper)
}

fn endpoint(value: &BigFloat, direction: Round) -> Result<Extended, Error> {
    if let Some(error) = value.err() {
        return Err(error);
    }
    let (words, precision, sign, exponent, _) =
        value.as_raw_parts().ok_or(Error::InvalidArgument)?;
    if precision != 128 || words.len() != 2 || sign != Sign::Pos || words[1] >> 63 != 1 {
        return Err(Error::InvalidArgument);
    }
    let significand = u128::from(words[0]) | (u128::from(words[1]) << 64);
    let bound = Quad::from_u128_r(significand, direction).value.scalbn_r(
        exponent.checked_sub(128).ok_or(Error::InvalidArgument)?,
        direction,
    );
    Ok(bound.convert_r(Round::NearestTiesToEven, &mut false).value)
}

fn certificate(lower: &BigFloat, upper: &BigFloat) -> Result<Option<u128>, Error> {
    let lower = endpoint(lower, Round::TowardNegative)?;
    let upper = endpoint(upper, Round::TowardPositive)?;
    Ok((lower.to_bits() == upper.to_bits()).then(|| lower.to_bits()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambiguous_and_extreme_enclosures() {
        let value = |low, high, exponent| {
            BigFloat::from_raw_parts(&[low, high], 128, Sign::Pos, exponent, false)
        };
        // A midpoint and the next 128-bit number straddle binary80 rounding.
        let lower = value(1 << 63, 1 << 63, 1);
        let upper = value((1 << 63) + 1, 1 << 63, 1);
        assert_eq!(certificate(&lower, &upper).unwrap(), None);
        let tiny = value(0, 1 << 63, -20000);
        assert_eq!(certificate(&tiny, &tiny).unwrap(), Some(0));
        let huge = value(0, 1 << 63, 20000);
        assert_eq!(
            certificate(&huge, &huge).unwrap(),
            Some(Extended::INFINITY.to_bits())
        );
        let invalid = BigFloat::from_raw_parts(&[1], 65, Sign::Pos, 0, false);
        assert_eq!(certificate(&invalid, &upper), Err(Error::InvalidArgument));
        let failed = BigFloat::nan(Some(Error::MemoryAllocation));
        assert_eq!(certificate(&failed, &upper), Err(Error::MemoryAllocation));
        assert_eq!(
            exp(&failed, &mut Consts::new().unwrap()),
            Err(Error::MemoryAllocation)
        );
    }
}
