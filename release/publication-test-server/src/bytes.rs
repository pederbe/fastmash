//! Bounded-memory comparison of packaged files.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

pub fn same(left: &Path, right: &Path) -> io::Result<bool> {
    let mut left = BufReader::new(File::open(left)?);
    let mut right = BufReader::new(File::open(right)?);
    loop {
        let left_bytes = left.fill_buf()?;
        let right_bytes = right.fill_buf()?;
        if left_bytes.is_empty() || right_bytes.is_empty() {
            return Ok(left_bytes.is_empty() && right_bytes.is_empty());
        }
        let length = left_bytes.len().min(right_bytes.len());
        if left_bytes[..length] != right_bytes[..length] {
            return Ok(false);
        }
        left.consume(length);
        right.consume(length);
    }
}
