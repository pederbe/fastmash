#![cfg(windows)]
#![forbid(unsafe_code)]
use fastmash_corpus_driver::{
    Options,
    fixture::{digest, encode},
    run,
};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(cases: &[Value]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fastmash-windows-driver-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_fastmash-corpus-test-command"),
            root.join("command.exe"),
        )
        .unwrap();
        let mut content = Vec::new();
        for case in cases {
            serde_json::to_writer(&mut content, case).unwrap();
            content.push(b'\n');
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&content).unwrap();
        let compressed = gzip.finish().unwrap();
        fs::write(root.join("cases.jsonl.gz"), &compressed).unwrap();
        fs::write(root.join("cases-provenance.json"), serde_json::to_vec(&json!({"cases": cases.len(), "fixture_sha256":digest(&compressed), "content_sha256":digest(&content)})).unwrap()).unwrap();
        Self { root }
    }
    fn options(&self, file: bool) -> Options {
        let mut options = Options::new(self.root.join("command.exe"));
        options.fixture = self.root.join("cases.jsonl.gz");
        options.stdin_file = file;
        options.evidence_dir = Some(self.root.join(if file { "file" } else { "pipe" }));
        options.keep_going = true;
        options
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
fn case(id: &str, action: &str, input: &[u8], stdout: &[u8], stderr: &[u8], status: i32) -> Value {
    json!({"id":id,"io":"normal","locale":"C","args":[action],"input_hex":encode(input),"expected":{"returncode":status,"stdout_hex":encode(stdout),"stderr_hex":encode(stderr)}})
}

#[test]
fn exact_bytes_and_mismatches_are_checked_in_both_input_modes() {
    let bytes = b"\0\r\x1a\xff\xc3\xa9\n";
    let fixture = Fixture::new(&[
        case("binary", "echo", bytes, bytes, b"", 0),
        case("output", "wrong-output", b"", b"expected\n", b"", 0),
        case("error", "wrong-error", b"", b"", b"expected\n", 0),
        case("status", "wrong-status", b"", b"", b"", 0),
    ]);
    for file in [false, true] {
        let result = run(&fixture.options(file), &mut Vec::new()).unwrap();
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, ["output", "error", "status"]);
    }
}

#[test]
fn overflow_and_blocked_input_have_bounded_job_cleanup() {
    let fixture = Fixture::new(&[
        case("overflow", "overflow", b"", b"", b"", 0),
        case("timeout", "stall", &vec![b'x'; 1 << 20], b"", b"", 0),
    ]);
    let started = Instant::now();
    let options = fixture.options(false);
    let result = run(&options, &mut Vec::new()).unwrap();
    assert_eq!(result.failed, ["overflow", "timeout"]);
    assert!(started.elapsed() < Duration::from_secs(15));
    let records =
        fs::read_to_string(options.evidence_dir.unwrap().join("observations.jsonl")).unwrap();
    let records = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records[0]["observed"]["overflow"], true);
    assert_eq!(records[1]["observed"]["timed_out"], true);
    assert!(
        records
            .iter()
            .all(|record| record["observed"]["cleanup_confirmed"] == true)
    );
}

#[test]
fn descendants_are_cancelled_after_the_leader_exits() {
    let mut value = case("descendant", "descendant", b"", b"", b"", 0);
    let fixture = Fixture::new(&[]);
    let marker = fixture.root.join("escaped");
    value["args"] = json!(["descendant", marker]);
    // Recreate only the fixture manifest, preserving the same owned directory.
    let mut content = serde_json::to_vec(&value).unwrap();
    content.push(b'\n');
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&content).unwrap();
    let compressed = gzip.finish().unwrap();
    fs::write(fixture.root.join("cases.jsonl.gz"), &compressed).unwrap();
    fs::write(fixture.root.join("cases-provenance.json"), serde_json::to_vec(&json!({"cases":1,"fixture_sha256":digest(&compressed),"content_sha256":digest(&content)})).unwrap()).unwrap();
    let result = run(&fixture.options(false), &mut Vec::new()).unwrap();
    assert_eq!(result.passed, 1);
    std::thread::sleep(Duration::from_millis(2200));
    assert!(!marker.exists(), "descendant escaped its Job");
}

#[test]
fn shards_cover_the_same_manifest_once_without_changing_expectations() {
    let cases = (0..5)
        .map(|index| {
            case(
                &format!("case-{index}"),
                "echo",
                b"x\0\xff\n",
                b"x\0\xff\n",
                b"",
                0,
            )
        })
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&cases);
    let mut seen = std::collections::BTreeSet::new();
    for part in 0..2 {
        let mut options = fixture.options(false);
        let evidence = fixture.root.join(format!("shard-{part}"));
        options.shard = Some((part, 2));
        options.evidence_dir = Some(evidence.clone());
        let result = run(&options, &mut Vec::new()).unwrap();
        assert_eq!(result.passed, if part == 0 { 3 } else { 2 });
        assert!(result.failed.is_empty());
        for line in fs::read_to_string(evidence.join("cases.jsonl"))
            .unwrap()
            .lines()
        {
            let value: Value = serde_json::from_str(line).unwrap();
            assert!(seen.insert(value["id"].as_str().unwrap().to_owned()));
        }
    }
    assert_eq!(seen, (0..5).map(|index| format!("case-{index}")).collect());
    let mut invalid = fixture.options(false);
    invalid.shard = Some((0, 0));
    assert!(run(&invalid, &mut Vec::new()).is_err());
}

#[test]
fn held_file_output_remains_a_real_regular_file() {
    let mut value = case("held-file", "file-echo", b"1\n", b"1\n", b"", 0);
    value["observe_held"] = json!(true);
    value["expected"]["held"] = json!({"alive":true,"bytes":0});
    let mut second = value.clone();
    second["id"] = json!("second-held-file");
    let pipe = case("pipe-rejected", "file-echo", b"1\n", b"1\n", b"", 0);
    let fixture = Fixture::new(&[value, second, pipe]);
    let options = fixture.options(false);
    let result = run(&options, &mut Vec::new()).unwrap();
    assert_eq!(result.passed, 2);
    assert_eq!(result.failed, ["pipe-rejected"]);
    let records =
        fs::read_to_string(options.evidence_dir.unwrap().join("observations.jsonl")).unwrap();
    for line in records.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        if record["id"] != "pipe-rejected" {
            assert_eq!(record["observed"]["destination"]["kind"], "regular-file");
        }
    }
}
