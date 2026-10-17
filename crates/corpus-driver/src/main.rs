#![forbid(unsafe_code)]

fn main() {
    match fastmash_corpus_driver::main_entry(std::env::args_os().skip(1)) {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("fastmash-corpus: {error}");
            std::process::exit(2);
        }
    }
}
