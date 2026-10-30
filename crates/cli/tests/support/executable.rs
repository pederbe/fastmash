//! Select the local Cargo executable or an installed command for integration tests.
use std::ffi::OsString;

pub fn fastmash() -> OsString {
    std::env::var_os("FASTMASH_TEST_BINARY").unwrap_or_else(|| {
        match option_env!("CARGO_BIN_EXE_fastmash") {
            Some(path) => path.into(),
            None => panic!("Cargo executable is only available to integration tests"),
        }
    })
}
