//! Reproducible complete Commands for local static profiling.
#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, File},
    io::Write,
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
};

struct Job {
    name: &'static str,
    args: &'static [&'static str],
    input: Vec<u8>,
    stdout: &'static [u8],
    stderr: &'static [u8],
    status: i32,
}

fn jobs() -> Vec<Job> {
    let grouped = b"b\t1\na\t1\n".repeat(32);
    vec![
        Job {
            name: "small",
            args: &["sum", "1"],
            input: b"1\n2\n3\n".to_vec(),
            stdout: b"6\n",
            stderr: b"",
            status: 0,
        },
        Job {
            name: "decimal",
            args: &["sum", "1"],
            input: b"0.1\n".repeat(1_000_000),
            stdout: b"100000\n",
            stderr: b"",
            status: 0,
        },
        Job {
            name: "grouping",
            args: &["-s", "-g1", "sum", "2"],
            input: grouped.clone(),
            stdout: b"a\t32\nb\t32\n",
            stderr: b"",
            status: 0,
        },
        Job {
            name: "spill",
            args: &["-s", "-g1", "sum", "2"],
            input: grouped,
            stdout: b"a\t32\nb\t32\n",
            stderr: b"",
            status: 0,
        },
        Job {
            name: "error",
            args: &["sum", "0"],
            input: Vec::new(),
            stdout: b"",
            stderr: b"fastmash: invalid field '0' for operation 'sum'\n",
            status: 1,
        },
        Job {
            name: "refusal",
            args: &["-s", "-g1", "sum", "1"],
            input: Vec::new(),
            stdout: b"",
            stderr: b"fastmash: FASTMASH_GROUPING must be sort or hash\n",
            status: 77,
        },
    ]
}

fn capture(root: &Path, binary: &Path, job: Job) -> Result<(), Box<dyn Error>> {
    let input = root.join(format!("{}.input", job.name));
    fs::write(&input, &job.input)?;
    let report = root.join(format!("{}.json", job.name));
    let spill = job.name == "spill";
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LC_ALL", "C")
        .envs(std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HOTPATH_")))
        .env("HOTPATH_OUTPUT_PATH", &report)
        .env("HOTPATH_OUTPUT_FORMAT", "json")
        .env("HOTPATH_METRICS_SERVER_OFF", "1")
        .env(
            "FASTMASH_GROUPING",
            if job.status == 77 { "invalid" } else { "sort" },
        )
        .env(
            "FASTMASH_SORT_MEMORY_BYTES",
            if spill { "1" } else { "67108864" },
        )
        .args(job.args)
        .stdin(File::open(&input)?);
    fs::write(
        root.join(format!("{}.command", job.name)),
        format!("{command:?}\n"),
    )?;
    let output = command.output()?;
    fs::write(root.join(format!("{}.stdout", job.name)), &output.stdout)?;
    fs::write(root.join(format!("{}.stderr", job.name)), &output.stderr)?;
    fs::write(
        root.join(format!("{}.status", job.name)),
        format!("{}\n", output.status),
    )?;
    if output.status.code() != Some(job.status)
        || output.stdout != job.stdout
        || output.stderr != job.stderr
    {
        return Err(format!("{} changed Command output or status: {output:?}", job.name).into());
    }
    let contents = fs::read_to_string(report)?;
    if !contents.starts_with('{') || !contents.contains("fastmash::Command") {
        return Err(format!("{} did not produce a Command report", job.name).into());
    }
    // This function is called only when a real private Spill run is opened.
    if spill && !contents.contains("projected_spill::temporary") {
        return Err("Spill job did not open a private run".into());
    }
    println!(
        "{}: exact streams and status, static report captured",
        job.name
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = args
        .next()
        .ok_or("usage: profile-jobs NEW-DIRECTORY [JOB]")?;
    let selected = args.next();
    if args.next().is_some() {
        return Err("usage: profile-jobs NEW-DIRECTORY [JOB]".into());
    }
    let jobs: Vec<_> = jobs()
        .into_iter()
        .filter(|job| selected.as_ref().is_none_or(|name| name == job.name))
        .collect();
    if jobs.is_empty() {
        return Err("unknown job, use small, decimal, grouping, spill, error or refusal".into());
    }
    fs::create_dir(&destination)?;
    let root = fs::canonicalize(destination)?;
    let executable = std::env::current_exe()?;
    let binary = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("example must be in the Cargo examples directory")?
        .join("fastmash");
    let mut identity = File::create(root.join("identity.txt"))?;
    writeln!(identity, "binary={}", binary.display())?;
    write!(identity, "sha256=")?;
    for byte in Sha256::digest(fs::read(&binary)?) {
        write!(identity, "{byte:02x}")?;
    }
    writeln!(
        identity,
        "\nmode=shared diagnostics\nfeatures: timing={} allocation={} cpu={}",
        cfg!(feature = "hotpath"),
        cfg!(feature = "hotpath-alloc"),
        cfg!(feature = "hotpath-cpu")
    )?;
    for job in jobs {
        capture(&root, &binary, job)?;
    }
    Ok(())
}
