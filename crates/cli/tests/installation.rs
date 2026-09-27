//! Run explicitly against a Cargo-installed pair, outside the checkout.
use std::{fs, path::Path, process::Command};

fn invoke(binary: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(fs::File::open(cwd.join("input")).unwrap())
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires FASTMASH_INSTALL_PREFIX pointing at a real Cargo installation"]
fn installed_pair_and_missing_companion() {
    let prefix = std::path::PathBuf::from(
        std::env::var_os("FASTMASH_INSTALL_PREFIX").expect("installed prefix"),
    );
    let binary = prefix.join("bin/fastmash");
    assert!(prefix.join("bin/fastmash-sort-supervisor").is_file());
    let cwd = std::env::temp_dir().join(format!("fastmash-installed-{}", std::process::id()));
    fs::create_dir(&cwd).unwrap();
    fs::write(cwd.join("input"), b"1\n2\n3\n").unwrap();
    let help = invoke(&binary, &cwd, &["--help"]);
    assert!(help.status.success());
    assert_eq!(help.stdout, include_bytes!("../src/help.txt"));
    assert!(help.stderr.is_empty());
    let version = invoke(&binary, &cwd, &["--version"]);
    assert!(version.status.success());
    assert_eq!(
        version.stdout,
        concat!("fastmash ", env!("CARGO_PKG_VERSION"), "\n").as_bytes()
    );
    assert!(version.stderr.is_empty());
    let sum = invoke(&binary, &cwd, &["sum", "1"]);
    assert!(sum.status.success());
    assert_eq!(sum.stdout, b"6\n");
    let median = invoke(&binary, &cwd, &["median", "1"]);
    assert!(median.status.success());
    assert_eq!(median.stdout, b"2\n");

    // A standalone copy exercises the user-visible partial-install failure.
    let alone = cwd.join("fastmash");
    fs::copy(&binary, &alone).unwrap();
    fs::write(cwd.join("input"), b"b\t2\t3\na\t1\t2\na\t3\t6\n").unwrap();
    let complete = invoke(&binary, &cwd, &["-sg1", "pcov", "2:3"]);
    assert!(complete.status.success(), "{complete:?}");
    assert_eq!(complete.stdout, b"a\t2\nb\t0\n");
    let missing = invoke(&alone, &cwd, &["-sg1", "pcov", "2:3"]);
    assert_eq!(missing.status.code(), Some(77), "{missing:?}");
    assert!(missing.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&missing.stderr),
        format!(
            "{}: unable to start sort supervisor\n\
             hint: install fastmash-sort-supervisor in the same directory as fastmash\n",
            alone.display()
        )
    );
    let independent = invoke(&alone, &cwd, &["sum", "2"]);
    assert!(independent.status.success());
    assert_eq!(independent.stdout, b"6\n");
    fs::remove_dir_all(cwd).unwrap();
}

#[test]
#[ignore = "requires FASTMASH_INSTALL_PREFIX pointing at a real Cargo installation"]
fn installed_migration_examples() {
    let prefix = std::path::PathBuf::from(
        std::env::var_os("FASTMASH_INSTALL_PREFIX").expect("installed prefix"),
    );
    let binary = prefix.join("bin/fastmash");
    let cwd = std::env::temp_dir().join(format!("fastmash-migration-{}", std::process::id()));
    fs::create_dir(&cwd).unwrap();
    for (args, input, expected) in [
        (vec!["sum", "1", "mean", "2"], "1\t2\n3\t4\n", "4\t3\n"),
        (
            vec!["-sg1", "sum", "2"],
            "b\t2\na\t1\na\t3\n",
            "a\t4\nb\t2\n",
        ),
        (
            vec!["-g1", "sum", "2"],
            "a\t1\na\t3\nb\t2\n",
            "a\t4\nb\t2\n",
        ),
        (
            vec!["-H", "sum", "reading", "mean", "reading"],
            "site\treading\nwest\t2\neast\t4\n",
            "sum(reading)\tmean(reading)\n6\t3\n",
        ),
        (
            vec!["--header-in", "sum", "reading", "mean", "reading"],
            "site\treading\nwest\t2\neast\t4\n",
            "6\t3\n",
        ),
        (vec!["perc:100", "1"], "1\n2\n3\n", "3\n"),
        (
            vec!["--narm", "dotprod", "1:2"],
            "1\tNA\nNA\t10\n2\t20\n",
            "50\n",
        ),
        (vec!["base64", "1"], "abc\n", "YWJj\n"),
    ] {
        fs::write(cwd.join("input"), input).unwrap();
        let output = invoke(&binary, &cwd, &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
    fs::write(cwd.join("input"), "1,5\n2,5\n").unwrap();
    let output = Command::new(&binary)
        .args(["sum", "1"])
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "de_DE.UTF-8")
        .stdin(fs::File::open(cwd.join("input")).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"4\n");
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(cwd).unwrap();
}
