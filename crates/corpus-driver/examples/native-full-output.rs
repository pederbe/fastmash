#![forbid(unsafe_code)]

//! Exercise real ENOSPC against the actual executable, without an interposer.
use std::{error::Error, path::PathBuf};
fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let candidate = PathBuf::from(arguments.next().ok_or("require executable")?);
    let root = PathBuf::from(arguments.next().ok_or("require new capture directory")?);
    let volume = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err("unexpected argument".into());
    }
    drop(fastmash_corpus_driver::full_output::qualify(
        &candidate,
        &root,
        volume.as_deref(),
    )?);
    println!("ordinary and actual full-output cases matched");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("native-full-output: {error}");
        std::process::exit(1);
    }
}
