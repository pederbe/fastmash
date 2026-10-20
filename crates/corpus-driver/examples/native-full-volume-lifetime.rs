#![forbid(unsafe_code)]
//! Qualify Rust ownership of native full-output images, including panic cleanup.
use fastmash_corpus_driver::full_volume;
use std::{error::Error, fs, path::PathBuf, process::Command};
fn run() -> Result<(), Box<dyn Error>> {
    if !cfg!(panic = "unwind") {
        return Err("build this lifetime control with CARGO_PROFILE_RELEASE_PANIC=unwind".into());
    }
    let mut arguments = std::env::args_os().skip(1);
    let candidate = PathBuf::from(arguments.next().ok_or("require candidate")?).canonicalize()?;
    let root = PathBuf::from(arguments.next().ok_or("require new captures")?);
    if arguments.next().is_some() {
        return Err("unexpected argument".into());
    }
    fs::create_dir(&root)?;
    fs::write(root.join("panic-profile"), b"unwind\n")?;
    let probe = std::env::current_exe()?.with_file_name("native-full-output");
    for unwind in [false, true] {
        let directory = root.join(if unwind { "unwind" } else { "normal" });
        let result = std::panic::catch_unwind(|| -> Result<(), Box<dyn Error>> {
            let volume = full_volume::FullVolume::new(&directory)?;
            let status = full_volume::bounded(
                Command::new(&probe)
                    .arg(&candidate)
                    .arg(directory.join("observations"))
                    .arg(volume.mount()),
                &directory,
                "candidate-probe",
                std::time::Duration::from_secs(90),
            )?;
            if !status.success() {
                return Err("actual full-output observations failed".into());
            }
            if unwind {
                panic!("selected lifetime-control unwind");
            }
            drop(volume);
            Ok(())
        });
        match (unwind, result) {
            (false, Ok(result)) => result?,
            (true, Err(error))
                if error.downcast_ref::<&str>() == Some(&"selected lifetime-control unwind") => {}
            (_, Ok(Err(error))) => return Err(error),
            _ => return Err("unexpected unwind result".into()),
        }
        if !directory.join("cleanup.ok").is_file()
            || directory.join("mount").try_exists()?
            || directory.join("full.dmg").try_exists()?
        {
            return Err("owned volume lifetime cleanup differs".into());
        }
    }
    // The corpus owner must also close its selected sink before detaching,
    // both on ordinary completion and while unwinding.
    for unwind in [false, true] {
        let directory = root.join(if unwind {
            "owned-unwind"
        } else {
            "owned-normal"
        });
        let result = std::panic::catch_unwind(|| -> Result<(), Box<dyn Error>> {
            let owner =
                fastmash_corpus_driver::full_output::FullOutput::new(&candidate, &directory)?;
            if unwind {
                panic!("selected corpus-owner unwind");
            }
            owner.finish()?;
            Ok(())
        });
        match (unwind, result) {
            (false, Ok(result)) => result?,
            (true, Err(error))
                if error.downcast_ref::<&str>() == Some(&"selected corpus-owner unwind") => {}
            (_, Ok(Err(error))) => return Err(error),
            _ => return Err("unexpected corpus-owner unwind result".into()),
        }
        if !directory.join("volume/cleanup.ok").is_file()
            || directory.join("volume/mount").try_exists()?
            || directory.join("volume/full.dmg").try_exists()?
        {
            return Err("corpus full-output lifetime cleanup differs".into());
        }
    }
    println!("ordinary and unwind lifetime controls matched actual full-output observations");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("native-full-volume-lifetime: {error}");
        std::process::exit(1);
    }
}
