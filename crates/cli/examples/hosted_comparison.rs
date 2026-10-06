//! Representative same-runner comparisons, scoped to the hosted machine.
//! Usage: hosted_comparison FASTMASH GNU WORK_DIRECTORY [RECORDS] [SAMPLES]
//! Output eligibility is checked before timing; no release threshold is implied.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Instant,
};

fn hash(path: &Path) -> io::Result<String> {
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let read = input.read(&mut bytes)?;
        if read == 0 {
            break;
        }
        digest.update(&bytes[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn run(binary: &Path, args: &[&str], input: &Path) -> io::Result<Output> {
    Command::new(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(File::open(input)?)
        .output()
}

fn timed(binary: &Path, args: &[&str], input: &Path, output: &Path) -> io::Result<f64> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(File::open(input)?)
        .stdout(File::create(output)?)
        .stderr(Stdio::piped());
    let begin = Instant::now();
    let result = command.spawn()?.wait_with_output()?;
    let elapsed = begin.elapsed().as_secs_f64();
    if !result.status.success() || !result.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "{}: {:?}",
            binary.display(),
            result
        )));
    }
    Ok(elapsed)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(3..=5).contains(&args.len()) {
        return Err(
            "usage: hosted_comparison FASTMASH GNU WORK_DIRECTORY [RECORDS] [SAMPLES]".into(),
        );
    }
    let candidate = fs::canonicalize(&args[0])?;
    let reference = fs::canonicalize(&args[1])?;
    let work = PathBuf::from(&args[2]);
    fs::create_dir_all(&work)?;
    let records: usize = args
        .get(3)
        .map_or(Ok(250_000), |a| a.to_string_lossy().parse())?;
    let samples: usize = args.get(4).map_or(Ok(7), |a| a.to_string_lossy().parse())?;
    if records == 0 || !(3..=31).contains(&samples) {
        return Err("records must be positive and samples between 3 and 31".into());
    }
    println!("scope\thosted runner, these generated jobs and settings only");
    println!("settings\tLC_ALL=C\tregular-file stdin\trecords={records}\tsamples={samples}");
    for (role, binary) in [("candidate", &candidate), ("reference", &reference)] {
        let version = Command::new(binary).arg("--version").output()?;
        if !version.status.success() {
            return Err(format!("{role} version failed: {version:?}").into());
        }
        println!(
            "identity\t{role}\t{}\tsha256={}\t{}",
            binary.display(),
            hash(binary)?,
            String::from_utf8_lossy(&version.stdout)
                .trim()
                .replace('\n', " | ")
        );
    }
    for (tool, arguments) in [
        ("uname", vec!["-a"]),
        ("sw_vers", vec![]),
        ("rustc", vec!["-Vv"]),
        ("sysctl", vec!["hw.model", "hw.ncpu", "hw.memsize"]),
    ] {
        match Command::new(tool).args(arguments).output() {
            Ok(output) => println!(
                "host\t{tool}\t{}\t{}",
                output.status,
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .replace('\n', " | ")
            ),
            Err(error) => println!("host\t{tool}\tunavailable: {error}"),
        }
    }
    let arithmetic = work.join("arithmetic.tsv");
    let grouped = work.join("grouped.tsv");
    {
        let mut numbers = io::BufWriter::new(File::create(&arithmetic)?);
        let mut groups = io::BufWriter::new(File::create(&grouped)?);
        let decimals = [
            "0", "0.125", "0.25", "0.375", "0.5", "0.625", "0.75", "0.875",
        ];
        for record in 0..records {
            writeln!(numbers, "{}\t{}", record % 32 + 1, decimals[record % 8])?;
            writeln!(
                groups,
                "site-{:04}\t{}",
                record.wrapping_mul(73) % 1024,
                record % 31 + 1
            )?;
        }
    }
    for (name, input, arguments) in [
        ("sum-and-mean", &arithmetic, vec!["sum", "1", "mean", "2"]),
        ("grouped-sum", &grouped, vec!["-sg1", "sum", "2"]),
        ("grouped-median", &grouped, vec!["-sg1", "median", "2"]),
    ] {
        println!(
            "fixture\t{name}\tsha256={}\targs={}",
            hash(input)?,
            arguments.join(" ")
        );
        let expected = run(&candidate, &arguments, input)?;
        let observed = run(&reference, &arguments, input)?;
        let eligible = expected.status.success()
            && observed.status.success()
            && expected.stderr.is_empty()
            && observed.stderr.is_empty()
            && expected.stdout == observed.stdout;
        fs::write(
            work.join(format!("{name}.candidate.stdout")),
            &expected.stdout,
        )?;
        fs::write(
            work.join(format!("{name}.reference.stdout")),
            &observed.stdout,
        )?;
        fs::write(
            work.join(format!("{name}.candidate.stderr")),
            &expected.stderr,
        )?;
        fs::write(
            work.join(format!("{name}.reference.stderr")),
            &observed.stderr,
        )?;
        println!(
            "eligibility\t{name}\t{eligible}\tcandidate_status={}\treference_status={}",
            expected.status, observed.status
        );
        if !eligible {
            continue;
        }
        let output = work.join(format!("{name}.timed.stdout"));
        let mut candidate_samples = Vec::new();
        let mut reference_samples = Vec::new();
        // A warmup for each executable, followed by alternating first runners.
        timed(&candidate, &arguments, input, &output)?;
        timed(&reference, &arguments, input, &output)?;
        for sample in 0..samples {
            for (role, binary) in if sample % 2 == 0 {
                [("candidate", &candidate), ("reference", &reference)]
            } else {
                [("reference", &reference), ("candidate", &candidate)]
            } {
                let elapsed = timed(binary, &arguments, input, &output)?;
                if fs::read(&output)? != expected.stdout {
                    return Err(format!("{name} {role} timed output changed").into());
                }
                println!("sample\t{name}\t{role}\t{}\t{elapsed:.9}", sample + 1);
                if role == "candidate" {
                    candidate_samples.push(elapsed);
                } else {
                    reference_samples.push(elapsed);
                }
            }
        }
        for (role, values) in [
            ("candidate", &mut candidate_samples),
            ("reference", &mut reference_samples),
        ] {
            values.sort_by(f64::total_cmp);
            let median = values[values.len() / 2];
            println!(
                "variation\t{name}\t{role}\tmin={:.9}\tmedian={median:.9}\tmax={:.9}\tspread_over_median={:.6}",
                values[0],
                values[values.len() - 1],
                (values[values.len() - 1] - values[0]) / median
            );
        }
        fs::remove_file(output)?;
    }
    Ok(())
}
