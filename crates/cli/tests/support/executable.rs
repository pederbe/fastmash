//! Select the local Cargo executable or an installed pair for integration tests.
use std::{ffi::OsString, path::PathBuf};

pub fn fastmash() -> OsString {
    std::env::var_os("FASTMASH_TEST_BINARY").unwrap_or_else(|| {
        match option_env!("CARGO_BIN_EXE_fastmash") {
            Some(path) => path.into(),
            None => panic!("Cargo executable is only available to integration tests"),
        }
    })
}

#[allow(dead_code)]
pub fn supervisor() -> OsString {
    std::env::var_os("FASTMASH_TEST_BINARY")
        .map(|binary| {
            PathBuf::from(binary)
                .with_file_name("fastmash-sort-supervisor")
                .into_os_string()
        })
        .unwrap_or_else(
            || match option_env!("CARGO_BIN_EXE_fastmash-sort-supervisor") {
                Some(path) => path.into(),
                None => panic!("Cargo supervisor is only available to integration tests"),
            },
        )
}
