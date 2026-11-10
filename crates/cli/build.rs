//! Select musl's portable copy routines for Linux release and source builds.
#![forbid(unsafe_code)]

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=vendor/musl");
    println!("cargo::rerun-if-env-changed=CC");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("musl") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    let compiler = env::var_os("CC").unwrap_or_else(|| "cc".into());
    for routine in ["memcpy", "memmove"] {
        let object = out.join(format!("{routine}.o"));
        let status = Command::new(&compiler)
            .args([
                "-O3",
                "-march=x86-64",
                "-mtune=generic",
                "-ffreestanding",
                "-fno-builtin",
            ])
            .arg(format!("-D{routine}=__wrap_{routine}"))
            .arg("-c")
            .arg(format!("vendor/musl/{routine}.c"))
            .arg("-o")
            .arg(&object)
            .status()
            .expect("musl builds need a C compiler (cc, or set CC)");
        assert!(status.success(), "could not compile musl {routine}");
        println!("cargo::rustc-link-arg-bin=fastmash={}", object.display());
        println!("cargo::rustc-link-arg-bin=fastmash=-Wl,--wrap={routine}");
    }
}
