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
    stdout: Vec<u8>,
    stderr: &'static [u8],
    status: i32,
}

fn jobs() -> Vec<Job> {
    let grouped = b"b\t1\na\t1\n".repeat(32);
    let mut jobs = vec![
        Job {
            name: "small",
            args: &["sum", "1"],
            input: b"1\n2\n3\n".to_vec(),
            stdout: b"6\n".to_vec(),
            stderr: b"",
            status: 0,
        },
        Job {
            name: "decimal",
            args: &["sum", "1"],
            input: b"0.125\n".repeat(10_000_000),
            stdout: b"1250000\n".to_vec(),
            stderr: b"",
            status: 0,
        },
        Job {
            name: "grouping",
            args: &["-s", "-g1", "sum", "2"],
            input: grouped.clone(),
            stdout: b"a\t32\nb\t32\n".to_vec(),
            stderr: b"",
            status: 0,
        },
        Job {
            name: "spill",
            args: &["-s", "-g1", "sum", "2"],
            input: grouped,
            stdout: b"a\t32\nb\t32\n".to_vec(),
            stderr: b"",
            status: 0,
        },
        Job {
            name: "error",
            args: &["sum", "0"],
            input: Vec::new(),
            stdout: Vec::new(),
            stderr: b"fastmash: invalid field '0' for operation 'sum'\n",
            status: 1,
        },
        Job {
            name: "refusal",
            args: &["-s", "-g1", "sum", "1"],
            input: Vec::new(),
            stdout: Vec::new(),
            stderr: b"fastmash: FASTMASH_GROUPING must be sort or hash\n",
            status: 77,
        },
    ];
    let suffix = "école".repeat(32);
    let mut input = String::new();
    let mut stdout = String::new();
    for at in 0..8192 {
        input.push_str(&format!("k{:02}{suffix}\t1\n", at % 16));
    }
    for group in 0..16 {
        stdout.push_str(&format!("k{group:02}{suffix}\t512\t1\n"));
    }
    jobs.push(Job {
        name: "language-cache",
        args: &["-s", "-g1", "count", "2", "rms", "2"],
        input: input.into_bytes(),
        stdout: stdout.into_bytes(),
        stderr: b"",
        status: 0,
    });
    // Each Group has equally many -1/+1 signals and -2/+2 factors.
    // Thus population skewness is 0, covariance is 2 and RMS is 1.
    // With 100,000 Records per Group, Jarque-Bera's p-value rounds to 0.
    let calculations = b"0\tb\t-1\t-2\n1\ta\t-1\t-2\n2\tb\t1\t2\n3\ta\t1\t2\n".repeat(50_000);
    for (name, args, stdout) in [
        (
            "pskew",
            &["-s", "-g2", "pskew", "3"][..],
            &b"a\t0\nb\t0\n"[..],
        ),
        (
            "jarque",
            &["-s", "-g2", "jarque", "3"][..],
            &b"a\t0\nb\t0\n"[..],
        ),
        (
            "pcov",
            &["-s", "-g2", "pcov", "3:4"][..],
            &b"a\t2\nb\t2\n"[..],
        ),
        (
            "mixed",
            &["-s", "-g2", "rms", "3", "pcov", "3:4", "last", "1"][..],
            &b"a\t1\t2\t3\nb\t1\t2\t2\n"[..],
        ),
    ] {
        jobs.push(Job {
            name,
            args,
            input: calculations.clone(),
            stdout: stdout.to_vec(),
            stderr: b"",
            status: 0,
        });
    }
    for (name, record) in [
        ("csv-short", b"a,b,c,1\n".to_vec()),
        ("csv-long", [vec![b'x'; 1024], b",1\n".to_vec()].concat()),
        (
            "csv-quoted",
            [
                b"\"".to_vec(),
                vec![b'x'; 1024],
                b",\r\nend\",1\r\n".to_vec(),
            ]
            .concat(),
        ),
        ("csv-escaped", b"\"a\"\"b\",\"c\"\"d\",,1\n".to_vec()),
    ] {
        jobs.push(Job {
            name,
            args: &["--csv-in", "count", "2"],
            input: record.repeat(4096),
            stdout: b"4096\n".to_vec(),
            stderr: b"",
            status: 0,
        });
    }
    // The same complete Records distinguish decoding from chunk retention and
    // selection. A tied rank retains only the first winner in both top jobs.
    let payload = vec![b'x'; 1024];
    let record = [b"a,1,\"".as_slice(), &payload, b"\"\n"].concat();
    let winner = [b"a\t1\t".as_slice(), &payload, b"\n"].concat();
    for (name, args, stdout) in [
        (
            "csv-copy",
            &["--csv-in", "count", "2"][..],
            b"4096\n".to_vec(),
        ),
        (
            "csv-copy-sort",
            &["--csv-in", "-s", "-g1", "count", "2"][..],
            b"a\t4096\n".to_vec(),
        ),
        (
            "csv-copy-spill",
            &["--csv-in", "-s", "-g1", "count", "2"][..],
            b"a\t4096\n".to_vec(),
        ),
        (
            "csv-copy-top",
            &["--csv-in", "top:1", "2"][..],
            winner.clone(),
        ),
        (
            "csv-copy-sort-top",
            &["--csv-in", "-s", "-g1", "top:1", "2"][..],
            winner,
        ),
    ] {
        jobs.push(Job {
            name,
            args,
            input: record.repeat(4096),
            stdout,
            stderr: b"",
            status: 0,
        });
    }
    jobs.push(Job {
        name: "csv-copy-wide",
        args: &["--csv-in", "count", "2"],
        input: [b"\"x\",".repeat(128), b"1\n".to_vec()]
            .concat()
            .repeat(4096),
        stdout: b"4096\n".to_vec(),
        stderr: b"",
        status: 0,
    });
    jobs
}

fn capture(root: &Path, binary: &Path, job: Job) -> Result<(), Box<dyn Error>> {
    let input = root.join(format!("{}.input", job.name));
    fs::write(&input, &job.input)?;
    let report = root.join(format!("{}.json", job.name));
    let spill = matches!(job.name, "spill" | "csv-copy-spill");
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
            match job.name {
                "spill" => "1",
                "csv-copy-spill" => "65536",
                _ => "67108864",
            },
        )
        .args(job.args)
        .stdin(File::open(&input)?);
    if job.name == "language-cache" {
        command
            .env("LC_ALL", "en_US.UTF-8")
            .env("OMP_NUM_THREADS", "1")
            .env("OMP_THREAD_LIMIT", "1");
    }
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
    if spill && !contents.contains("projected_sort::spill::temporary") {
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
        return Err("unknown job, use small, decimal, grouping, spill, error, refusal, language-cache, pskew, jarque, pcov, mixed, csv-short, csv-long, csv-quoted, csv-escaped, csv-copy, csv-copy-sort, csv-copy-spill, csv-copy-top, csv-copy-sort-top or csv-copy-wide".into());
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
