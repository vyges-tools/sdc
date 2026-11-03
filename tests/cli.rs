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
    assert!(v["not_written"].to_string().contains("set_data_check"), "{v}");
    assert!(!std::path::Path::new(&o).exists(), "a refused write must leave no file");
    // the text form says it too, on stderr
    let (rc, _, err) = sdc(&["write", "--sdc", "tests/data/unmodelled.sdc", "-o", &o]);
    assert_eq!(rc, 1);
    assert!(err.contains("REFUSED") && err.contains("set_data_check"), "{err}");
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
    assert!(text.contains("# not written:") && text.contains("set_data_check"), "{text}");
}

#[test]
fn describe_parses_and_names_both_tools() {
    let (rc, out, _) = sdc(&["--describe"]);
    assert_eq!(rc, 0);
    let v = json(&out);
    assert_eq!(v["name"], "sdc");
    assert_eq!(v["tools"][0]["name"], "sdc.write");
}

fn sdc_env(args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_vyges-sdc"));
    c.args(args).current_dir(env!("CARGO_MANIFEST_DIR")).env_remove("VYGES_LOG");
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c.output().expect("run vyges-sdc");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// Exit 2 is "could not read the input", kept apart from exit 1, a finding about constraints
/// that were read — a gate must not read a missing file as a constraint failure, or the reverse.
#[test]
fn an_unreadable_input_exits_2_with_an_input_event() {
    let mut a = vec!["check", "--sdc", "tests/data/no-such.sdc", "--json"];
    a.extend(DESIGN);
    let (rc, out, err) = sdc_env(&a, &[("VYGES_LOG_FORMAT", "json")]);
    assert_eq!(rc, 2);
    assert_eq!(json(&out)["status"], "error");
    assert!(err.contains("\"SDC-INPUT\""), "{err}");
}

#[test]
fn fail_on_warning_turns_a_warning_into_exit_1() {
    let mut a = vec!["check", "--sdc", "tests/data/no_out.sdc", "--fail-on-warning"];
    a.extend(DESIGN);
    assert_eq!(sdc(&a).0, 1);
}

/// Every finding is an event with its stable code; the summary follows.
#[test]
fn each_finding_is_an_event_keyed_by_its_code() {
    let mut a = vec!["check", "--sdc", "tests/data/no_out.sdc"];
    a.extend(DESIGN);
    let (_, _, err) = sdc_env(&a, &[("VYGES_LOG_FORMAT", "json")]);
    let codes: Vec<String> = err
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_string))
        .collect();
    assert!(codes.contains(&"SDC-UNCONSTRAINED-OUTPUT".to_string()), "{codes:?}");
    assert_eq!(codes.last().map(String::as_str), Some("SDC-CHECK"), "{codes:?}");
}

/// ⛔ The suite's verbosity contract (vyges-cli `docs/src/output-and-logging.md`): `-q` warn,
/// `-qq` error, `-qqq` off, `-v` debug; a flag OVERRIDES `VYGES_LOG`; `VYGES_LOG` takes a name or
/// `0`-`5`, `off` included. And a parsed-and-ignored flag fails silently, so every step is seen.
#[test]
fn verbosity_follows_the_suite_contract() {
    let mut a = vec!["check", "--sdc", "tests/data/no_out.sdc"];
    a.extend(DESIGN);
    let run = |extra: &[&str], log: Option<&str>| {
        let mut x = a.clone();
        x.extend(extra);
        let mut env = vec![("VYGES_LOG_FORMAT", "json")];
        if let Some(l) = log {
            env.push(("VYGES_LOG", l));
        }
        sdc_env(&x, &env).2
    };
    let warn = "SDC-UNCONSTRAINED-OUTPUT"; // a warning
    let base = run(&[], None);
    assert!(base.contains(warn) && !base.contains("SDC-READ"), "{base}");
    assert!(run(&["-q"], None).contains(warn), "-q keeps warnings");
    assert!(!run(&["-qq"], None).contains(warn), "-qq is errors only");
    assert!(!run(&["-q", "--quiet"], None).contains(warn), "repeats combine, long form too");
    assert_eq!(run(&["-qqq"], None).trim(), "", "-qqq is silent");
    assert!(run(&["-v"], None).contains("SDC-READ"), "-v is debug");
    assert!(run(&[], Some("debug")).contains("SDC-READ"), "VYGES_LOG alone sets it");
    assert!(run(&[], Some("4")).contains("SDC-READ"), "numeric VYGES_LOG");
    assert_eq!(run(&[], Some("off")).trim(), "", "VYGES_LOG=off is silent");
    assert!(!run(&["-qq"], Some("debug")).contains(warn), "a flag beats VYGES_LOG");
}

#[test]
fn version_names_the_commit_and_links_point_at_the_central_tracker() {
    let (rc, out, _) = sdc(&["--version"]);
    assert_eq!(rc, 0);
    assert!(out.starts_with("vyges-sdc ") && out.contains('('), "{out}");
    let (rc, out, _) = sdc(&["--bug-report"]);
    assert_eq!(rc, 0);
    assert!(out.contains("github.com/vyges/community"), "{out}");
    assert!(sdc(&["--star"]).1.contains("vyges-tools/sdc"));
}

/// The help documents every flag the engine reads (a flag only the code knows is a flag no
/// user finds).
#[test]
fn help_documents_every_flag() {
    let help = sdc(&["--help"]).1;
    for f in [
        "--sdc", "--netlist", "--lib", "--metadata", "--fail-on-warning", "--out", "--design",
        "--partial", "--json", "--quiet", "--verbose", "--describe", "--bug-report",
        "--feature-request", "--sponsor", "--star", "VYGES_LOG", "VYGES_LOG_FORMAT", "EXIT STATUS",
    ] {
        assert!(help.contains(f), "{f}");
    }
}

/// The commands `write` emits for `tests/data/supported.sdc`, one of each it writes.
fn written_commands() -> std::collections::BTreeSet<String> {
    let o = scratch("supported.sdc");
    let (rc, _, err) = sdc(&["write", "--sdc", "tests/data/supported.sdc", "-o", &o, "-qqq"]);
    assert_eq!(rc, 0, "{err}");
    std::fs::read_to_string(&o)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

/// **The README's constraint list is exactly what `write` writes**: a command added to the
/// writer and not to the README fails, and so does one the README names that is not written.
#[test]
fn the_readme_lists_exactly_what_write_writes() {
    let readme = include_str!("../README.md");
    let block = readme
        .split("<!-- constraints-written:begin -->")
        .nth(1)
        .and_then(|r| r.split("<!-- constraints-written:end -->").next())
        .expect("README constraints block");
    let listed: std::collections::BTreeSet<String> = block
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    assert_eq!(listed, written_commands());
}

/// The README's other claims: synthesis directives are read past (what follows is written, and
/// each is listed as not written), an unmodelled command is named, a command that is not SDC
/// stops the read (exit 2, the command named), and both spellings of "every input but the
/// clock" are read.
#[test]
fn the_readme_claims_about_reading_hold() {
    let dir = scratch("claims");
    std::fs::create_dir_all(&dir).unwrap();
    let run = |name: &str, text: &str, extra: &[&str]| {
        let f = format!("{dir}/{name}.sdc");
        std::fs::write(&f, format!("create_clock -name clk -period 10 [get_ports clk]\n{text}\n")).unwrap();
        let o = format!("{dir}/{name}.out.sdc");
        let mut a = vec!["write", "--sdc", f.as_str(), "-o", o.as_str(), "--json", "-qqq"];
        a.extend(extra);
        let (rc, out, err) = sdc(&a);
        (rc, out, err, std::fs::read_to_string(&o).unwrap_or_default())
    };
    let synth = "set_size_only [get_cells u0]\nset_critical_range 0.2 [current_design]\nset_dont_touch_network [get_ports clk]\n\
                 set_optimize_design\nset_scan_configuration -chain_count 1\nset_min_porosity 50 [current_design]\n\
                 set_clock_gating_enable\nset_clock_gating_style -sequential_cell latch\nset_clock_gating_verification\n\
                 set_timing_enable_verification\nset_load 0.01 [get_ports dout]";
    let (rc, out, err, w) = run("synth", synth, &["--partial"]);
    assert_eq!(rc, 0, "{err}");
    assert!(w.contains("set_load 0.01"), "{out}\n{w}");
    assert_eq!(json(&out)["not_written"].as_array().map(Vec::len), Some(10), "{out}");
    let (rc, out, _, _) = run("unmodelled", "set_data_check -from [get_ports din] -to [get_ports dout] 0.1", &[]);
    assert_eq!(rc, 1);
    assert!(json(&out)["not_written"].to_string().contains("set_data_check"), "{out}");
    let (rc, out, err, _) = run("unknown", "derive_pll_clocks", &[]);
    assert_eq!(rc, 2, "{err}");
    assert!(json(&out)["error"].as_str().is_some_and(|e| e.contains("invalid command name \"derive_pll_clocks\"")), "{out}");
    for (name, ports) in [
        ("lsearch", "[lsearch -inline -all -not -exact [all_inputs] [get_ports clk]]"),
        ("remove", "[remove_from_collection [all_inputs] clk]"),
    ] {
        let (rc, out, _, w) = run(name, &format!("set_input_delay 1 -clock clk {ports}"), &[]);
        assert_eq!(rc, 0, "{name}: {out}");
        assert!(w.contains("[lsearch -inline -all -not -exact [all_inputs] [get_ports {clk}]]"), "{name}: {w}");
    }
}

/// Without a netlist, `check` runs the checks the file alone supports: a clean file exits 0 with a
/// note that the design checks did not run; a timing command not applied is an error (exit 1).
#[test]
fn check_without_a_netlist_reports_what_was_not_applied() {
    let (rc, out, err) = sdc(&["check", "--sdc", "tests/data/clean.sdc", "--json", "-qqq"]);
    assert_eq!(rc, 0, "{err}");
    assert!(json(&out)["findings"].to_string().contains("design-checks-not-run"), "{out}");
    let (rc, out, _) = sdc(&["check", "--sdc", "tests/data/unmodelled.sdc", "--json", "-qqq"]);
    assert_eq!(rc, 1, "{out}");
    let v = json(&out);
    assert!(v["findings"].to_string().contains("sdc-not-applied") && v["findings"].to_string().contains("set_data_check"), "{v}");
}

/// `--define NAME=VALUE` sets a variable before the file is read: a period the flow supplies.
#[test]
fn a_define_is_set_before_the_file_is_read() {
    let dir = scratch("define");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/d.sdc");
    std::fs::write(&f, "create_clock -name clk -period [expr 10*$F] [get_ports clk]\n").unwrap();
    let (rc, _, _) = sdc(&["check", "--sdc", &f, "-qqq"]);
    assert_eq!(rc, 2);
    let o = format!("{dir}/d.out.sdc");
    let (rc, _, err) = sdc(&["write", "--sdc", &f, "-o", &o, "--define", "F=0.5", "-qqq"]);
    assert_eq!(rc, 0, "{err}");
    assert!(std::fs::read_to_string(&o).unwrap().contains("-period 5"));
    let (rc, _, _) = sdc(&["check", "--sdc", &f, "--define", "F"]);
    assert_eq!(rc, 2);
}
