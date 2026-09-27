use super::*;

#[test]
#[ignore = "requires installed candidate, named GNU reference and fresh evidence directory"]
fn enhancement_decision_current_workflows() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    // Current behavior only. Proposed extension syntax is deliberately not tested.
    let cases = vec![
        (
            args(&["-H", "sum", "reading", "mean", "reading"]),
            b"site\treading\nwest\t2\neast\t4\n".to_vec(),
        ),
        (
            args(&["--header-in", "sum", "reading", "mean", "reading"]),
            b"site\treading\nwest\t2\neast\t4\n".to_vec(),
        ),
        (
            args(&["-t,", "-H", "sum", "reading"]),
            b"site,reading\n\"lab,west\",2\n\"lab,east\",4\n".to_vec(),
        ),
        (
            args(&["-H", "mean", "sample_A,sample_B"]),
            b"id\tsample_A\tsample_B\n1\t2\t6\n2\t4\t8\n".to_vec(),
        ),
        (
            args(&["-H", "mean", "sample_*"]),
            b"id\tsample_A\tsample_B\n1\t2\t6\n2\t4\t8\n".to_vec(),
        ),
    ];
    compare_cases(&cases, &reference, &evidence);
}
