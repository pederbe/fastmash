//! Native directory and glibc query boundary for the development generator.

use crate::{Reference, Result, collect, parse_numeric};
use serde_json::json;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USAGE: &str = "Usage: fastmash-generate-numeric-locales [--ldd PATH] [--locale PATH] [--uname PATH] [--] LOCPATH OUT_JSON OUT_RS";
const QUERY_LIMIT: Duration = Duration::from_secs(30);
const FIXED_PATH: &str = "/usr/bin:/bin";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tools {
    pub ldd: PathBuf,
    pub locale: PathBuf,
    pub uname: PathBuf,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            ldd: "/usr/bin/ldd".into(),
            locale: "/usr/bin/locale".into(),
            uname: "/usr/bin/uname".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub locpath: PathBuf,
    pub out_json: PathBuf,
    pub out_rs: PathBuf,
    pub tools: Tools,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Summary {
    pub locales: usize,
    pub refused: usize,
}

/// Parse paths as operating-system strings. Locale names are never options.
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>> {
    let mut args = args.into_iter();
    let mut tools = Tools::default();
    let mut paths = Vec::new();
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            if arg == "--help" || arg == "-h" {
                return Ok(None);
            }
            if arg == "--" {
                positional = true;
                continue;
            }
            let tool = if arg == "--ldd" {
                Some(&mut tools.ldd)
            } else if arg == "--locale" {
                Some(&mut tools.locale)
            } else if arg == "--uname" {
                Some(&mut tools.uname)
            } else {
                None
            };
            if let Some(tool) = tool {
                *tool = args.next().ok_or("a tool option requires a path")?.into();
                continue;
            }
            if arg.as_bytes().starts_with(b"-") {
                return Err(format!("unknown option: {}", arg.to_string_lossy()).into());
            }
        }
        paths.push(PathBuf::from(arg));
    }
    let [locpath, out_json, out_rs]: [PathBuf; 3] = paths
        .try_into()
        .map_err(|_| "expected LOCPATH OUT_JSON OUT_RS")?;
    if out_json == out_rs {
        return Err("OUT_JSON and OUT_RS must be different files".into());
    }
    Ok(Some(Config {
        locpath,
        out_json,
        out_rs,
        tools,
    }))
}

/// Query the selected native tools and write only the caller's output files.
/// Their version text is metadata, not proof of reference provenance.
pub fn generate(config: &Config) -> Result<Summary> {
    let evidence = private_directory("fastmash-locale-query")?;
    let result = generate_in(config, &evidence);
    match result {
        Ok(summary) => {
            fs::remove_dir_all(&evidence).map_err(|error| {
                format!(
                    "cannot clean successful query evidence {}: {error}",
                    evidence.display()
                )
            })?;
            Ok(summary)
        }
        Err(error) => {
            let _ = fs::write(evidence.join("failure.txt"), format!("{error}\n"));
            Err(format!("{error} (native query evidence: {})", evidence.display()).into())
        }
    }
}

fn generate_in(config: &Config, evidence: &Path) -> Result<Summary> {
    let locpath = fs::canonicalize(&config.locpath)
        .map_err(|error| format!("cannot read LOCPATH {}: {error}", config.locpath.display()))?;
    let directories = directories(&locpath)?;
    let mut queries = Queries {
        root: evidence,
        locpath: &locpath,
        next: 0,
        limit: QUERY_LIMIT,
    };
    let version = queries.run(&config.tools.ldd, &["--version"], "C")?;
    let glibc = clean_text(&version)?
        .lines()
        .next()
        .filter(|line| !line.is_empty())
        .ok_or("ldd returned no version line")?
        .to_owned();
    let architecture = queries.run(&config.tools.uname, &["-m"], "C")?;
    let machine = clean_text(&architecture)?.trim_end_matches(['\r', '\n']);
    if machine.is_empty() || machine.contains(['\r', '\n']) {
        return Err("uname did not return one nonempty machine line".into());
    }
    let snapshot = collect(
        Reference {
            glibc,
            machine: machine.to_owned(),
        },
        directories,
        |name| {
            let reply = queries.run(&config.tools.locale, &["-k", "LC_NUMERIC"], name)?;
            parse_numeric(clean_text(&reply)?)
        },
    )?;
    let tables = snapshot.tables()?;
    let raw = snapshot.json()?;
    let source = snapshot.rust_source()?;
    fs::write(&config.out_json, raw)
        .map_err(|error| format!("cannot write {}: {error}", config.out_json.display()))?;
    fs::write(&config.out_rs, source)
        .map_err(|error| format!("cannot write {}: {error}", config.out_rs.display()))?;
    Ok(Summary {
        locales: tables.numeric.len(),
        refused: tables.refused.len(),
    })
}

fn directories(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.path().is_dir() {
            names.push(
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "a locale directory name is not valid UTF-8")?,
            );
        }
    }
    Ok(names)
}

fn private_directory(prefix: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()));
    DirBuilder::new().mode(0o700).create(&path)?;
    Ok(path)
}

struct Queries<'a> {
    root: &'a Path,
    locpath: &'a Path,
    next: usize,
    limit: Duration,
}

struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Queries<'_> {
    fn run(&mut self, program: &Path, args: &[&str], locale: &str) -> Result<Captured> {
        self.next += 1;
        let directory = self.root.join(format!("query-{:04}", self.next));
        DirBuilder::new().mode(0o700).create(&directory)?;
        fs::write(
            directory.join("command.json"),
            serde_json::to_vec_pretty(&json!({
                "program": program.to_string_lossy(),
                "program_bytes": program.as_os_str().as_bytes(),
                "arguments": args,
                "environment_cleared": true,
                "environment": {
                    "PATH": FIXED_PATH,
                    "LC_ALL": locale,
                    "LOCPATH_bytes": self.locpath.as_os_str().as_bytes(),
                },
                "limit_milliseconds": self.limit.as_millis(),
            }))?,
        )?;
        let capture = |name| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join(name))
        };
        let stdout = capture("stdout")?;
        let stderr = capture("stderr")?;
        let started = Instant::now();
        let spawned = Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", FIXED_PATH)
            .env("LC_ALL", locale)
            .env("LOCPATH", self.locpath)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                fs::write(
                    directory.join("wait.json"),
                    serde_json::to_vec_pretty(&json!({
                        "spawn_error": error.to_string(), "pid": null,
                        "raw_status": null, "exit_code": null, "signal": null,
                        "reaped": false, "timed_out": false,
                        "kill_error": null, "wait_error": null,
                    }))?,
                )?;
                return Err(error.into());
            }
        };
        let pid = child.id();
        let waited = wait_bounded(&mut child, started, self.limit);
        fs::write(
            directory.join("wait.json"),
            serde_json::to_vec_pretty(&json!({
                "pid": pid,
                "raw_status": waited.status.map(ExitStatusExt::into_raw),
                "exit_code": waited.status.and_then(|status| status.code()),
                "signal": waited.status.and_then(|status| status.signal()),
                "reaped": waited.status.is_some(),
                "timed_out": waited.timed_out,
                "kill_error": waited.kill_error,
                "wait_error": waited.wait_error,
            }))?,
        )?;
        if waited.timed_out || waited.wait_error.is_some() || waited.status.is_none() {
            return Err(format!(
                "native query did not complete cleanly (reaped: {})",
                waited.status.is_some()
            )
            .into());
        }
        Ok(Captured {
            status: waited.status.ok_or("native query has no terminal status")?,
            stdout: fs::read(directory.join("stdout"))?,
            stderr: fs::read(directory.join("stderr"))?,
        })
    }
}

fn clean_text(reply: &Captured) -> Result<&str> {
    if !reply.status.success() {
        return Err(format!("native query exited with {}", reply.status).into());
    }
    if !reply.stderr.is_empty() {
        return Err("native query wrote a warning or diagnostic to stderr".into());
    }
    Ok(std::str::from_utf8(&reply.stdout)?)
}

struct Waited {
    status: Option<ExitStatus>,
    timed_out: bool,
    kill_error: Option<String>,
    wait_error: Option<String>,
}

fn wait_bounded(child: &mut Child, started: Instant, limit: Duration) -> Waited {
    let reserve = Duration::from_secs(1).min(limit / 4);
    let cancel_after = limit - reserve;
    let mut result = Waited {
        status: None,
        timed_out: false,
        kill_error: None,
        wait_error: None,
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                result.status = Some(status);
                result.timed_out |= started.elapsed() >= limit;
                return result;
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                // Ownership is uncertain. Never signal a possibly reaped PID.
                result.wait_error = Some(error.to_string());
                return result;
            }
        }
        let elapsed = started.elapsed();
        if !result.timed_out && elapsed >= cancel_after {
            result.timed_out = true;
            if let Err(error) = child.kill() {
                result.kill_error = Some(error.to_string());
            }
        }
        if elapsed >= limit {
            result.wait_error =
                Some("direct-child cleanup did not finish within its reserve".into());
            return result;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn arguments_keep_paths_as_data_and_select_tools() {
        let args = [
            "--ldd",
            "/selected/ldd",
            "--locale",
            "/selected/locale",
            "--uname",
            "/selected/uname",
            "--",
            "-locales",
            "chosen.json",
            "chosen.rs",
        ];
        let config = parse_args(args.map(OsString::from)).unwrap().unwrap();
        assert_eq!(config.locpath, Path::new("-locales"));
        assert_eq!(config.out_json, Path::new("chosen.json"));
        assert_eq!(config.out_rs, Path::new("chosen.rs"));
        assert_eq!(config.tools.ldd, Path::new("/selected/ldd"));
        assert_eq!(config.tools.locale, Path::new("/selected/locale"));
        assert_eq!(config.tools.uname, Path::new("/selected/uname"));
        let ordinary = parse_args(["locales", "raw.json", "table.rs"].map(OsString::from))
            .unwrap()
            .unwrap();
        assert_eq!(ordinary.tools, Tools::default());
        assert!(parse_args([OsString::from("--help")]).unwrap().is_none());
        for args in [
            vec!["--ldd"],
            vec!["only-one"],
            vec!["--unknown"],
            vec!["a", "b", "b"],
        ] {
            assert!(parse_args(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[test]
    fn directory_names_are_exact_utf8_data_and_files_are_ignored() {
        let root = private_directory("fastmash-locale-directory-test").unwrap();
        let names = ["sr_RS.UTF-8@latin", "line\nbreak", "C.UTF-8", "fr_é"];
        for name in names {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(root.join("ordinary-file"), b"").unwrap();
        std::os::unix::fs::symlink(root.join("fr_é"), root.join("directory-link")).unwrap();
        std::os::unix::fs::symlink(root.join("absent"), root.join("broken-link")).unwrap();
        let mut found = directories(&root).unwrap();
        found.sort();
        let mut expected = names.map(str::to_owned).to_vec();
        expected.push("directory-link".into());
        expected.sort();
        assert_eq!(found, expected);
        let invalid = root.join(OsString::from_vec(vec![b'x', 0xff]));
        fs::create_dir(invalid).unwrap();
        assert!(directories(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_transport_captures_exact_environment_errors_and_timeouts() {
        let root = private_directory("fastmash-locale-transport-test").unwrap();
        let mut queries = Queries {
            root: &root,
            locpath: &root,
            next: 0,
            limit: Duration::from_secs(2),
        };
        let unusual = "quote\"name\nfr_é";
        let reply = queries
            .run(Path::new("/usr/bin/printenv"), &["LC_ALL"], unusual)
            .unwrap();
        assert_eq!(clean_text(&reply).unwrap(), format!("{unusual}\n"));
        let declaration: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("query-0001/command.json")).unwrap())
                .unwrap();
        assert_eq!(declaration["environment"]["LC_ALL"], unusual);
        assert_eq!(declaration["environment_cleared"], true);
        let failed = queries
            .run(Path::new("/bin/ls"), &["--fastmash-invalid-option"], "C")
            .unwrap();
        assert!(!failed.status.success());
        assert!(!failed.stderr.is_empty());
        assert!(clean_text(&failed).is_err());
        let warning = Captured {
            status: ExitStatus::from_raw(0),
            stdout: b"decimal_point=\".\"\n".to_vec(),
            stderr: b"locale selection failed\n".to_vec(),
        };
        assert!(clean_text(&warning).is_err());
        assert!(queries.run(Path::new("/bin/sleep"), &["10"], "C").is_err());
        let timeout: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("query-0003/wait.json")).unwrap()).unwrap();
        assert_eq!(timeout["timed_out"], true);
        assert_eq!(timeout["reaped"], true);
        assert!(timeout["raw_status"].is_number());
        let stdout = fs::read(root.join("query-0002/stdout")).unwrap();
        let stderr = fs::read(root.join("query-0002/stderr")).unwrap();
        assert_eq!(stdout, failed.stdout);
        assert_eq!(stderr, failed.stderr);
        fs::remove_dir_all(root).unwrap();
    }
}
