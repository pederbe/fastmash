// Numerical candidate probe; never selected by the production CLI.
#[path = "../src/guarded_log.rs"]
mod guarded_log;
#[path = "../src/log_table.rs"]
mod log_table;
use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};
use rustc_apfloat::{Float, ieee::X87DoubleExtended as Extended};
use std::{hint::black_box, time::Instant};
fn main() {
    let mut constants = Consts::new().unwrap();
    let values: Vec<_> = (1..=20000)
        .map(|i| {
            Extended::from_str_r(
                &format!("{}.{:05}", 1 + i % 29, i),
                rustc_apfloat::Round::NearestTiesToEven,
            )
            .unwrap()
            .value
            .to_bits()
        })
        .collect();
    let start = Instant::now();
    let trial: Vec<_> = values
        .iter()
        .map(|&v| black_box(guarded_log::log(black_box(v))))
        .collect();
    println!("quad_ns\t{}", start.elapsed().as_nanos());
    let start = Instant::now();
    let reference: Vec<_> = values
        .iter()
        .map(|&v| {
            let value = BigFloat::from_raw_parts(
                &[v as u64],
                64,
                Sign::Pos,
                ((v >> 64) as i32) - 16382,
                false,
            );
            let result = value.ln(64, RoundingMode::ToEven, &mut constants);
            let (m, _, sign, e, _) = result.as_raw_parts().unwrap();
            (u128::from((e + 16382) as u16 | if sign == Sign::Neg { 0x8000 } else { 0 }) << 64)
                | u128::from(m[0])
        })
        .collect();
    println!("astro_ns\t{}", start.elapsed().as_nanos());
    let mut fallback = 0;
    for (actual, expected) in trial.iter().zip(reference) {
        if let Some(actual) = actual {
            assert_eq!(*actual, expected);
        } else {
            fallback += 1;
        }
    }
    println!(
        "cases\t{}\nfallback\t{}\nmismatch\t0",
        values.len(),
        fallback
    );
    if let Some(path) = std::env::args().nth(1) {
        let text = std::fs::read_to_string(path).unwrap();
        let wine: Vec<_> = text
            .lines()
            .skip(1)
            .map(|line| {
                Extended::from_str_r(
                    line.split(';').nth(10).unwrap(),
                    rustc_apfloat::Round::NearestTiesToEven,
                )
                .unwrap()
                .value
                .to_bits()
            })
            .collect();
        for (name, inputs) in [("wine", &wine), ("unique", &values)] {
            for backend in ["astro", "quad"] {
                for size in [0, 16, 64, 256, 1024] {
                    let mut cache = vec![(0u128, 0u128); size];
                    let mut misses = 0;
                    let start = Instant::now();
                    for &bits in inputs {
                        let mut hash = (bits as u64) ^ ((bits >> 64) as u64).rotate_left(17);
                        hash ^= hash >> 30;
                        hash = hash.wrapping_mul(0xbf58476d1ce4e5b9);
                        hash ^= hash >> 27;
                        hash = hash.wrapping_mul(0x94d049bb133111eb);
                        hash ^= hash >> 31;
                        let index = (hash as usize) & size.saturating_sub(1);
                        let value = if size != 0 && cache[index].0 == bits {
                            cache[index].1
                        } else {
                            misses += 1;
                            let value = if backend == "quad" {
                                guarded_log::log(bits).unwrap()
                            } else {
                                let input = BigFloat::from_raw_parts(
                                    &[bits as u64],
                                    64,
                                    Sign::Pos,
                                    ((bits >> 64) as i32) - 16382,
                                    false,
                                );
                                let result = input.ln(64, RoundingMode::ToEven, &mut constants);
                                let (m, _, sign, e, _) = result.as_raw_parts().unwrap();
                                (u128::from(
                                    (e + 16382) as u16 | if sign == Sign::Neg { 0x8000 } else { 0 },
                                ) << 64)
                                    | u128::from(m[0])
                            };
                            if size != 0 {
                                cache[index] = (bits, value);
                            }
                            value
                        };
                        black_box(value);
                    }
                    println!(
                        "cache\t{backend}\t{name}\t{size}\t{}\t{misses}\t{}",
                        inputs.len(),
                        start.elapsed().as_nanos()
                    );
                }
            }
        }
    }
}
