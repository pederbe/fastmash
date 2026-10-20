#![forbid(unsafe_code)]

fn main() {
    let output = std::process::Command::new("pkg-config")
        .args(["--modversion", "icu-i18n"])
        .output()
        .expect("ICU reference builds require pkg-config and ICU development files");
    assert!(output.status.success(), "pkg-config could not identify ICU");
    let version = std::str::from_utf8(&output.stdout).expect("ICU version must be UTF-8");
    let version = version.trim();
    assert!(
        !version.is_empty() && version.split('.').all(|part| part.parse::<u32>().is_ok()),
        "invalid ICU version"
    );
    println!("cargo:rustc-env=FASTMASH_REFERENCE_ICU={version}");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");
}
