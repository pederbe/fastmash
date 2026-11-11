//! Checked generated Commands used to train the Linux release compiler profile.
#![forbid(unsafe_code)]

use std::{
    error::Error,
    fs::{self, File},
    io::Write,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Job {
    name: &'static str,
    input: Vec<u8>,
    expected: Vec<u8>,
    args: &'static [&'static str],
    locale: &'static str,
    grouping: &'static str,
    memory: &'static str,
}

fn job(
    name: &'static str,
    input: Vec<u8>,
    expected: Vec<u8>,
    args: &'static [&'static str],
) -> Job {
    Job {
        name,
        input,
        expected,
        args,
        locale: "C",
        grouping: "hash",
        memory: "67108864",
    }
}

fn training() -> Vec<Job> {
    let mut grouped = Vec::new();
    for i in 0..32768 {
        writeln!(&mut grouped, "k{:02}\t1\tx", (i * 13) % 32).unwrap();
    }
    let mut text = Vec::new();
    let mut summary = Vec::new();
    for i in 0..32 {
        writeln!(&mut text, "k{i:02}\t1024\tx\tx\tx").unwrap();
        writeln!(&mut summary, "k{i:02}\t1024\t1024\t1").unwrap();
    }
    let mut jobs = vec![
        job(
            "tiny",
            b"1\n2\n3\n".to_vec(),
            b"6\n".to_vec(),
            &["sum", "1"],
        ),
        job(
            "decimal",
            b"0.125\n".repeat(100000),
            b"12500\t0.125\n".to_vec(),
            &["sum", "1", "mean", "1"],
        ),
        job(
            "retained",
            b"-1\n1\n".repeat(10000),
            b"0\t1\t-1\t1\n".to_vec(),
            &["median", "1", "pvar", "1", "min", "1", "max", "1"],
        ),
        job(
            "text",
            grouped.clone(),
            text,
            &[
                "-s", "-g1", "count", "2", "unique", "3", "first", "3", "last", "3",
            ],
        ),
        job(
            "hash",
            grouped.clone(),
            summary.clone(),
            &["-s", "-g1", "count", "2", "sum", "2", "rms", "2"],
        ),
        job(
            "sort",
            grouped,
            summary,
            &["-s", "-g1", "count", "2", "sum", "2", "rms", "2"],
        ),
        job(
            "paired",
            b"b\t-1\t-2\na\t-1\t-2\nb\t1\t2\na\t1\t2\n".repeat(8192),
            b"a\t1\t2\nb\t1\t2\n".to_vec(),
            &["-s", "-g1", "rms", "2", "pcov", "2:3"],
        ),
        job(
            "csv",
            b"b,1,\"x\"\"y\"\na,1,\"x\"\"y\"\n".repeat(4096),
            b"a\t4096\tx\"y\nb\t4096\tx\"y\n".to_vec(),
            &["--csv-in", "-s", "-g1", "count", "2", "unique", "3"],
        ),
        job(
            "locale",
            "z\t1\nä\t1\n".as_bytes().repeat(4096),
            "ä\t4096\nz\t4096\n".as_bytes().to_vec(),
            &["-s", "-g1", "count", "2"],
        ),
    ];
    jobs[5].grouping = "sort";
    jobs[5].memory = "65536";
    jobs[8].locale = "de_DE.UTF-8";
    jobs
}

fn digest(path: &Path) -> Result<String> {
    let output = Command::new("sha256sum").arg("--").arg(path).output()?;
    if !output.status.success() {
        return Err(format!("cannot hash {}", path.display()).into());
    }
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or("missing digest")?
        .into())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: fastmash-pgo-train BINARY NEW-TRAINING-DIRECTORY NEW-PROFILE-DIRECTORY".into(),
        );
    }
    let binary = Path::new(&args[0]).canonicalize()?;
    fs::create_dir(&args[1])?;
    let root = Path::new(&args[1]).canonicalize()?;
    fs::create_dir(&args[2])?;
    let profiles = Path::new(&args[2]).canonicalize()?;
    let mut manifest = File::create(root.join("manifest.tsv"))?;
    writeln!(manifest, "# executable_sha256={}", digest(&binary)?)?;
    writeln!(
        manifest,
        "job\tpass\tinput_sha256\texpected_sha256\tstdout_sha256\tstatus\tlocale\tgrouping\tsort_memory_bytes\targuments"
    )?;
    let mut commands = 0;
    for job in training() {
        let input = root.join(format!("{}.input", job.name));
        let expected = root.join(format!("{}.expected", job.name));
        fs::write(&input, job.input)?;
        fs::write(&expected, &job.expected)?;
        let input_hash = digest(&input)?;
        let expected_hash = digest(&expected)?;
        // Preserve the qualified mix's startup weight without dominating
        // the execution counts of record-processing paths.
        let passes = if job.name == "tiny" { 32 } else { 1 };
        for pass in 0..passes {
            let prefix = format!("{}-{pass}", job.name);
            let stdout = root.join(format!("{prefix}.stdout"));
            let stderr = root.join(format!("{prefix}.stderr"));
            let status = Command::new(&binary)
                .arg0("fastmash")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", job.locale)
                .env("TZ", "UTC")
                .env("NO_COLOR", "1")
                .env("FASTMASH_GROUPING", job.grouping)
                .env("FASTMASH_SORT_MEMORY_BYTES", job.memory)
                .env(
                    "LLVM_PROFILE_FILE",
                    profiles.join(format!("{prefix}-%m-%p.profraw")),
                )
                .args(job.args)
                .stdin(File::open(&input)?)
                .stdout(Stdio::from(File::create(&stdout)?))
                .stderr(Stdio::from(File::create(&stderr)?))
                .status()?;
            writeln!(
                manifest,
                "{}\t{pass}\t{input_hash}\t{expected_hash}\t{}\t{}\t{}\t{}\t{}\t{}",
                job.name,
                digest(&stdout)?,
                status.code().unwrap_or(-1),
                job.locale,
                job.grouping,
                job.memory,
                job.args.join(" ")
            )?;
            if !status.success()
                || fs::read(&stdout)? != job.expected
                || !fs::read(&stderr)?.is_empty()
            {
                return Err(format!(
                    "PGO training {} changed stdout, stderr or exit status ({status})",
                    job.name
                )
                .into());
            }
            commands += 1;
        }
    }
    println!("PGO training: {commands} commands checked with exact stdout, stderr and exit status");
    Ok(())
}
