//! The external-sort companion. Install it beside `fastmash`, which locates it
//! next to its own executable.
#[cfg(target_os = "linux")]
fn main() {
    fastmash_sort_process::supervisor::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("fastmash-sort-supervisor: external sorting requires Linux");
    std::process::exit(77);
}
