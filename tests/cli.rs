//! The command line, run as a user runs it: exit codes, the JSON envelope, and that `write`
//! never drops a command silently.

use std::process::Command;

fn sdc(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_vyges-sdc"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run vyges-sdc");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("{e}: {s}"))
}

fn scratch(name: &str) -> String {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name).to_string_lossy().into_owned()
}

const DESIGN: [&str; 4] = ["--netlist", "tests/data/t.v", "--lib", "tests/data/seq.lib"];

#[test]
fn check_of_whole_constraints_is_clean_and_exits_0() {
    let mut a = vec!["check", "--sdc", "tests/data/clean.sdc", "--json"];
    a.extend(DESIGN);
    let (rc, out, err) = sdc(&a);
    assert_eq!(rc, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["clean"], true);
    assert_eq!(v["errors"], 0);
    assert_eq!(v["endpoint_coverage_pct"], 100.0);
}

/// An unconstrained output is a warning, not an error: the check reports it and still exits 0.
#[test]
fn check_names_an_unconstrained_output() {
    let mut a = vec!["check", "--sdc", "tests/data/no_out.sdc"];
    a.extend(DESIGN);
    let (rc, out, _) = sdc(&a);
    assert_eq!(rc, 0);
    assert!(out.contains("dout"), "{out}");
    assert!(out.contains("warning"), "{out}");
}

#[test]
fn check_without_a_lib_is_a_usage_error() {
    let (rc, _, err) = sdc(&["check", "--sdc", "tests/data/clean.sdc", "--netlist", "tests/data/t.v"]);
    assert_eq!(rc, 2);
    assert!(err.contains("--lib"), "{err}");
}

#[test]
fn write_of_a_whole_file_is_complete() {
    let o = scratch("clean.out.sdc");
    let (rc, out, err) = sdc(&["write", "--sdc", "tests/data/clean.sdc", "-o", &o, "--json"]);
    assert_eq!(rc, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["complete"], true);
    assert_eq!(v["out"], o.as_str());
    let text = std::fs::read_to_string(&o).unwrap();
    assert!(text.contains("create_clock -name {clk} -period 10 [get_ports {clk}]"), "{text}");
    assert!(text.contains("set_output_delay"), "{text}");
}

/// ⛔ The property this engine exists for: a command the model does not carry is never dropped
/// in silence. Without --partial nothing is written and the exit is 1.
#[test]
fn write_refuses_when_a_command_would_be_dropped() {
    let o = scratch("refused.out.sdc");
    let _ = std::fs::remove_file(&o);
    let (rc, out, _) = sdc(&["write", "--sdc", "tests/data/unmodelled.sdc", "-o", &o, "--json"]);
    assert_eq!(rc, 1);
    let v = json(&out);
    assert_eq!(v["status"], "refused");
    assert_eq!(v["complete"], false);
    assert!(v["not_written"].to_string().contains("set_disable_timing"), "{v}");
    assert!(!std::path::Path::new(&o).exists(), "a refused write must leave no file");
    // the text form says it too, on stderr
    let (rc, _, err) = sdc(&["write", "--sdc", "tests/data/unmodelled.sdc", "-o", &o]);
    assert_eq!(rc, 1);
    assert!(err.contains("REFUSED") && err.contains("set_disable_timing"), "{err}");
}

#[test]
fn partial_write_marks_every_dropped_command_in_the_file() {
    let o = scratch("partial.out.sdc");
    let (rc, out, _) = sdc(&["write", "--sdc", "tests/data/unmodelled.sdc", "-o", &o, "--partial", "--json"]);
    assert_eq!(rc, 0);
    let v = json(&out);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["complete"], false);
    let text = std::fs::read_to_string(&o).unwrap();
    assert!(text.contains("# not written:") && text.contains("set_disable_timing"), "{text}");
}

#[test]
fn describe_parses_and_names_both_tools() {
    let (rc, out, _) = sdc(&["--describe"]);
    assert_eq!(rc, 0);
    let v = json(&out);
    assert_eq!(v["name"], "sdc");
    assert_eq!(v["tools"][0]["name"], "sdc.write");
}
