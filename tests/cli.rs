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
    assert!(v["not_written"].to_string().contains("set_assigned_delay"), "{v}");
    assert!(!std::path::Path::new(&o).exists(), "a refused write must leave no file");
    // the text form says it too, on stderr
    let (rc, _, err) = sdc(&["write", "--sdc", "tests/data/unmodelled.sdc", "-o", &o]);
    assert_eq!(rc, 1);
    assert!(err.contains("REFUSED") && err.contains("set_assigned_delay"), "{err}");
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
    assert!(text.contains("# not written:") && text.contains("set_assigned_delay"), "{text}");
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
    let (rc, out, _, _) = run("unmodelled", "set_assigned_delay -net -from [get_ports din] -to [get_ports dout] 0.1", &[]);
    assert_eq!(rc, 1);
    assert!(json(&out)["not_written"].to_string().contains("set_assigned_delay"), "{out}");
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
    assert!(v["findings"].to_string().contains("sdc-not-applied") && v["findings"].to_string().contains("set_assigned_delay"), "{v}");
}

/// Overlapping exceptions, decided by the reference's priority: a multicycle under a false path on
/// the same points never applies (a warning, exit 0); a second, later multicycle of the same
/// priority replaces the first; a false path inside another is redundant (information).
#[test]
fn check_reports_shadowed_conflicting_and_redundant_exceptions() {
    let dir = scratch("overlap");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/o.sdc");
    std::fs::write(
        &f,
        "create_clock -name clk -period 10 [get_ports clk]\n\
         set_false_path -from [get_pins r1/CK]\n\
         set_multicycle_path 3 -from [get_pins r1/CK]\n\
         set_multicycle_path 2 -through [get_pins u1/Y]\n\
         set_multicycle_path 4 -through [get_pins u1/Y]\n\
         set_false_path -from [get_pins r1/CK] -to [get_pins r2/D]\n",
    )
    .unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--json", "-qqq"]);
    assert_eq!(rc, 0, "warnings leave the exit code at 0: {err}{out}");
    let v = json(&out);
    let codes: Vec<(String, String)> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|x| x["code"].as_str().is_some_and(|c| c.starts_with("exception-")))
        .map(|x| (x["severity"].as_str().unwrap_or("").to_string(), x["code"].as_str().unwrap_or("").to_string()))
        .collect();
    let want = [("warning", "exception-conflict"), ("warning", "exception-shadowed"), ("info", "exception-redundant")];
    assert_eq!(codes, want.map(|(s, c)| (s.to_string(), c.to_string())).to_vec(), "{v}");
    let text = v["findings"].to_string();
    assert!(text.contains("set_multicycle_path 2 -through"), "the earlier one is the one replaced: {text}");
}

/// Clock groups read as the reference reads them: `-include_generated_clocks` is ignored (so the
/// divided clock is in no group and is timed against B), a group of patterns holds the clocks
/// they match, and two clocks of one family in different asynchronous groups are named.
#[test]
fn check_reports_clock_groups_that_do_not_say_what_they_appear_to() {
    let dir = scratch("clockgroups");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/g.sdc");
    std::fs::write(
        &f,
        "create_clock -name A -period 10 [get_ports clka]\n\
         create_clock -name B -period 7 [get_ports clkb]\n\
         create_generated_clock -name A_DIV2 -source [get_ports clka] -divide_by 2 [get_pins rdiv/Q]\n\
         set_clock_groups -asynchronous -include_generated_clocks -group A -group B\n\
         set_clock_groups -asynchronous -name fam -group A -group [get_clocks A_*]\n",
    )
    .unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--json", "-qqq"]);
    assert_eq!(rc, 0, "warnings leave the exit code at 0: {err}{out}");
    let v = json(&out);
    let codes: Vec<String> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|x| x["code"].as_str().filter(|c| c.starts_with("clock-") || *c == "sdc-word-ignored").map(String::from))
        .collect();
    assert_eq!(codes, ["clock-generated-ungrouped", "clock-related-declared-async", "sdc-word-ignored"], "{v}");
    let text = v["findings"].to_string();
    assert!(text.contains("times `A_DIV2` against `B`"), "{text}");
}

/// With the design, a generated clock's master is found as the reference finds it (a backward
/// search from its -source through wires, combinational arcs and register clock-to-Q), and its
/// waveform derived from it: G1 (no clock on its -source pin) is A / 2, G4 is G1 / 2 though
/// defined first (neither has a non-positive period), and G3, whose search reaches no clock, has
/// no master and no waveform.
#[test]
fn check_with_the_design_finds_each_generated_clocks_master() {
    let dir = scratch("genclk");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/g.sdc");
    std::fs::write(
        &f,
        "create_clock -name A -period 10 [get_ports clka]\n\
         create_generated_clock -name G4 -divide_by 2 -source [get_pins rdiv/Q] [get_pins r2/CK]\n\
         create_generated_clock -name G1 -divide_by 2 -source [get_pins rdiv/CK] [get_pins rdiv/Q]\n\
         create_generated_clock -name G3 -divide_by 2 -source [get_pins ux/Y] [get_pins r3/CK]\n",
    )
    .unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--netlist", "tests/data/genclk.v", "--lib", "tests/data/seq.lib", "--json", "-qqq"]);
    assert_eq!(rc, 1, "G3 has no waveform, an error: {err}{out}");
    let v = json(&out);
    let found: Vec<String> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|x| x["code"].as_str().is_some_and(|c| c.starts_with("clock-")))
        .map(|x| format!("{} {}", x["code"].as_str().unwrap(), x["message"].as_str().unwrap()))
        .collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("clock-period") && found[0].contains("`G3`"), "{found:?}");
    assert!(found[1].starts_with("clock-generated-no-master") && found[1].contains("`G3`"), "{found:?}");
    // G1 on rdiv/Q clocks r1, and r2 through a buffer; G3 is on r3's own clock pin, so no
    // register is untimed (G3's own error says it has no waveform)
    let untimed: Vec<&str> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|x| x["code"] == "register-no-clock-reaches")
        .filter_map(|x| x["message"].as_str())
        .collect();
    assert!(untimed.is_empty(), "{untimed:?}");
}

/// An exception whose `-from` query matches nothing on the design makes no exception (the
/// reference drops the whole command): reported, and not read as cutting every path into r.
#[test]
fn check_reports_an_exception_whose_query_matches_nothing() {
    let dir = scratch("emptyexc");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/e.sdc");
    std::fs::write(
        &f,
        "create_clock -name clk -period 10 [get_ports clk]\n\
         set_false_path -from [get_ports -quiet -filter {direction == inout} *] -to [get_cells r]\n",
    )
    .unwrap();
    let (_, out, err) = sdc(&["check", "--sdc", &f, "--netlist", "tests/data/t.v", "--lib", "tests/data/seq.lib", "--json", "-qqq"]);
    let v = json(&out);
    let m: Vec<&str> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|x| x["code"] == "exception-matches-nothing")
        .filter_map(|x| x["message"].as_str())
        .collect();
    assert_eq!(m.len(), 1, "{err}{v}");
    assert!(m[0].contains("STA-0471"), "{m:?}");
}

/// Waivers round trip: `--write-waivers` writes a template for the active findings, a filled-in
/// copy waives them all (listed as waived, counted as nothing), a removed waiver brings back
/// exactly its finding, a waiver for nothing is stale, and the template is never overwritten.
#[test]
fn waivers_written_then_read_waive_every_finding_and_only_those() {
    let dir = scratch("waivers");
    std::fs::create_dir_all(&dir).unwrap();
    let f = format!("{dir}/o.sdc");
    std::fs::write(
        &f,
        "create_clock -name clk -period 10 [get_ports clk]\n\
         set_false_path -from [get_pins r1/CK]\n\
         set_multicycle_path 3 -from [get_pins r1/CK]\n\
         set_multicycle_path 2 -through [get_pins u1/Y]\n\
         set_multicycle_path 4 -through [get_pins u1/Y]\n",
    )
    .unwrap();
    let t = format!("{dir}/template.tcl");
    let _ = std::fs::remove_file(&t);
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--write-waivers", &t, "--json", "-qqq"]);
    assert_eq!(rc, 0, "{err}{out}");
    let before = json(&out);
    assert_eq!(before["warnings"], 2, "{before}");
    let template = std::fs::read_to_string(&t).unwrap();
    assert_eq!(template.matches("waive_finding -id ").count(), 2, "{template}");
    // never over a file that exists
    let (rc, _, err) = sdc(&["check", "--sdc", &f, "--write-waivers", &t, "-qqq"]);
    assert_eq!(rc, 2, "{err}");
    assert_eq!(std::fs::read_to_string(&t).unwrap(), template);
    // an empty reason waives nothing, and says so
    let (_, out, _) = sdc(&["check", "--sdc", &f, "--waivers", &t, "--json", "-qqq"]);
    let v = json(&out);
    assert_eq!(v["waived"].as_array().map(Vec::len), Some(0), "{v}");
    assert!(v["findings"].to_string().contains("waiver-no-reason"), "{v}");
    // filled in: both waived, no warnings left
    let filled = format!("{dir}/filled.tcl");
    std::fs::write(&filled, template.replace("-reason {}", "-reason {reviewed: intended}")).unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--waivers", &filled, "--json", "-qqq"]);
    assert_eq!(rc, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["warnings"], 0, "{v}");
    assert_eq!(v["waived"].as_array().map(Vec::len), Some(2), "{v}");
    assert_eq!(v["waived"][0]["reason"], "reviewed: intended", "{v}");
    // each finding's id in the plain report is the one its waiver names
    let ids: Vec<String> = before["findings"].as_array().unwrap().iter().filter(|x| x["severity"] != "info").map(|x| x["id"].as_str().unwrap().to_string()).collect();
    for id in &ids {
        assert!(template.contains(&format!("-id {id} ")), "{id} in {template}");
    }
    // one removed: exactly its finding comes back
    let first = template.lines().find(|l| l.starts_with("waive_finding")).unwrap();
    let less = format!("{dir}/less.tcl");
    std::fs::write(&less, template.replace(first, "").replace("-reason {}", "-reason {ok}")).unwrap();
    let (_, out, _) = sdc(&["check", "--sdc", &f, "--waivers", &less, "--json", "-qqq"]);
    let v = json(&out);
    assert_eq!(v["warnings"], 1, "{v}");
    assert!(first.contains(v["findings"].as_array().unwrap().iter().find(|x| x["severity"] == "warning").unwrap()["id"].as_str().unwrap()), "{v}");
    // a waiver for a finding that is gone is stale
    let stale = format!("{dir}/stale.tcl");
    std::fs::write(&stale, "waive_finding -id exception-shadowed:000000000000 -code exception-shadowed -reason {fixed}\n").unwrap();
    let (_, out, _) = sdc(&["check", "--sdc", &f, "--waivers", &stale, "--json", "-qqq"]);
    assert!(out.contains("waiver-stale"), "{out}");
    // an unreadable waiver file is an input error
    let (rc, _, _) = sdc(&["check", "--sdc", &f, "--waivers", &format!("{dir}/no-such.tcl"), "-qqq"]);
    assert_eq!(rc, 2);
}

/// `waivers export`: with a template for our own format the result reads back as the same
/// waivers; with another form, an unmapped code is left out, listed, and exit is 1; the output
/// file is never overwritten; a template typo is an input error.
#[test]
fn waivers_export_writes_by_template_and_never_guesses_a_rule() {
    let dir = scratch("export");
    std::fs::create_dir_all(&dir).unwrap();
    let w = format!("{dir}/w.tcl");
    std::fs::write(
        &w,
        "waive_finding -id exception-shadowed:aaaaaaaaaaaa -code exception-shadowed -objects {{a/b}} -reason {intended}\n\
         waive_finding -id clock-period:cccccccccccc -code clock-period -objects {} -reason {r}\n",
    )
    .unwrap();
    let own = format!("{dir}/own.txt");
    std::fs::write(&own, "command waive_finding -id @id@ -code @code@ -objects {@objects@} -reason {@reason@}\nobject {@object@}\nrule * @code@\n").unwrap();
    let (rc, out, err) = sdc(&["waivers", "export", "--waivers", &w, "--template", &own, "-qqq"]);
    assert_eq!(rc, 0, "{err}");
    assert_eq!(out.matches("waive_finding -id ").count(), 2, "{out}");
    assert!(out.contains("-id exception-shadowed:aaaaaaaaaaaa -code exception-shadowed -objects {{a/b}} -reason {intended}"), "{out}");
    let other = format!("{dir}/other.txt");
    std::fs::write(&other, "command my_waive -rule {@rule@} -comment {@reason@}\nrule exception-shadowed OVERLAP_SHADOW\n").unwrap();
    let o = format!("{dir}/out.tcl");
    let _ = std::fs::remove_file(&o);
    let (rc, out, _) = sdc(&["waivers", "export", "--waivers", &w, "--template", &other, "-o", &o, "--json", "-qqq"]);
    assert_eq!(rc, 1, "an unmapped code is left out: {out}");
    let v = json(&out);
    assert_eq!(v["written"], 1, "{v}");
    assert!(v["skipped"].to_string().contains("clock-period"), "{v}");
    assert!(std::fs::read_to_string(&o).unwrap().contains("my_waive -rule {OVERLAP_SHADOW} -comment {intended}"));
    let (rc, _, _) = sdc(&["waivers", "export", "--waivers", &w, "--template", &other, "-o", &o, "-qqq"]);
    assert_eq!(rc, 2, "never over a file");
    let typo = format!("{dir}/typo.txt");
    std::fs::write(&typo, "command my_waive -rule {@rul@}\nrule * X\n").unwrap();
    let (rc, _, _) = sdc(&["waivers", "export", "--waivers", &w, "--template", &typo, "-qqq"]);
    assert_eq!(rc, 2);
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
    // undefined, the variable is Tcl's error, which stops the read (the README's message)
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--json"]);
    assert_eq!(rc, 2);
    assert!(format!("{out}{err}").contains("can't read \\\"F\\\": no such variable") || format!("{out}{err}").contains("can't read \"F\": no such variable"), "{out}{err}");
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    assert!(readme.contains("`can't read \"X\": no such variable`"));
}

/// With the libraries and a netlist, `[load_of lib/cell/pin]` is the library pin's capacitance
/// (as `set_load` takes it, in the library's unit); without them the command is not applied.
#[test]
fn load_of_reads_the_library_pin_with_the_libraries() {
    let f = scratch("load_of.sdc");
    std::fs::write(&f, "create_clock -name clk -period 10 [get_ports clk]\nset_load [load_of seq_demo/BUF/A] [get_ports dout]\n").unwrap();
    let o = scratch("load_of.out.sdc");
    let mut a = vec!["write", "--sdc", f.as_str(), "-o", o.as_str(), "-qqq"];
    a.extend(DESIGN);
    let (rc, out, err) = sdc(&a);
    assert_eq!(rc, 0, "{out}{err}");
    assert!(std::fs::read_to_string(&o).unwrap().contains("set_load 0.0015 [get_ports {dout}]"), "{}", std::fs::read_to_string(&o).unwrap());
    let (rc, out, _) = sdc(&["check", "--sdc", &f, "--json"]);
    assert_eq!(rc, 1, "{out}");
    assert!(out.contains("not evaluated: no library was given"), "{out}");
}

/// A hierarchical netlist is flattened before checking, as the timer reads it: an exception on
/// a pin inside a block (`u_b/r/D`) names an object that exists, and every register is clocked.
#[test]
fn a_hierarchical_netlist_is_flattened_for_check() {
    let (rc, out, err) = sdc(&["check", "--sdc", "tests/data/hier.sdc", "--netlist", "tests/data/hier.v", "--lib", "tests/data/seq.lib", "--json", "-qqq"]);
    assert_eq!(rc, 0, "{out}\n{err}");
    let v = json(&out);
    assert_eq!(v["endpoint_coverage_pct"], 100.0, "{v}");
    assert!(!v["findings"].to_string().contains("exception-"), "{v}");
    // a module kept as a block leaves its pins unknown to the design: the exception names nothing
    let (_, out, _) = sdc(&["check", "--sdc", "tests/data/hier.sdc", "--netlist", "tests/data/hier.v", "--lib", "tests/data/seq.lib", "--keep", "stage", "--json", "-qqq"]);
    assert!(json(&out)["findings"].to_string().contains("exception-unknown-object"), "{out}");
}

/// The signoff timer's take-back commands are read as the reference's undo: `remove_propagated_clock`
/// leaves the clock ideal, and `remove_input_delay` (no `-clock`) takes back every input delay on
/// its objects. With a loom that predates them (the pin this engine had before), the read stops at
/// `remove_propagated_clock` as an unknown command (exit 2); this test pins the loom that reads them.
#[test]
fn signoff_take_back_commands_are_read_not_refused() {
    let mut a = vec!["check", "--sdc", "tests/data/take_back.sdc", "--json"];
    a.extend(DESIGN);
    let (rc, out, err) = sdc(&a);
    assert_eq!(rc, 0, "{out}{err}");
    let v = json(&out);
    assert_eq!(v["errors"], 0, "{out}");
    let o = scratch("take_back.out.sdc");
    let (rc, out, err) = sdc(&["write", "--sdc", "tests/data/take_back.sdc", "-o", &o, "--json"]);
    assert_eq!(rc, 0, "{out}{err}");
    let text = std::fs::read_to_string(&o).unwrap();
    // the clock was made propagated and taken back: no `set_propagated_clock` is written
    assert!(!text.contains("set_propagated_clock"), "{text}");
    // the input delay was taken back, and the output delay stands
    assert!(!text.contains("set_input_delay"), "{text}");
    assert!(text.contains("set_output_delay"), "{text}");
}

/// A command kept and written back that no checker or timer here applies is reported by `check`,
/// with its count — a warning when it can move a slack (`set_clock_gating_check` adds a check on a
/// gating enable), info when it cannot (`set_max_area`) — so the file is not reported CLEAN. It
/// is not an error: the exit stays 0, and `write` writes it back.
#[test]
fn check_reports_each_kept_command_nothing_applies() {
    let mut a = vec!["check", "--sdc", "tests/data/kept.sdc", "--json"];
    a.extend(DESIGN);
    let (rc, out, err) = sdc(&a);
    assert_eq!(rc, 0, "{out}{err}");
    let v = json(&out);
    assert_eq!(v["kept_not_applied"], 2, "{out}");
    let kept: Vec<&serde_json::Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["code"] == "sdc-kept-not-applied").collect();
    assert_eq!(kept[0]["severity"], "warning", "{out}");
    assert!(kept[0]["message"].as_str().unwrap().contains("`set_clock_gating_check` (2 commands)"), "{out}");
    assert_eq!(kept[1]["severity"], "info", "{out}");
    let mut a = vec!["check", "--sdc", "tests/data/kept.sdc"];
    a.extend(DESIGN);
    let (_, text, _) = sdc(&a);
    assert!(!text.contains("CLEAN"), "{text}");
    let o = scratch("kept.out.sdc");
    let (rc, out, err) = sdc(&["write", "--sdc", "tests/data/kept.sdc", "-o", &o, "--json"]);
    assert_eq!(rc, 0, "{out}{err}");
    assert!(std::fs::read_to_string(&o).unwrap().contains("set_clock_gating_check"));
}

/// **The README's command coverage is generated, not written**: every command, its SDC version,
/// its status and the consumers that apply it come from loom's command registry, whose own test
/// holds each status to what the reader does with the command. A README edited by hand, or a
/// registry changed without the README, fails here; `cargo run --example coverage_table`
/// prints what belongs between the markers.
#[test]
fn the_readme_coverage_tables_are_the_registry() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    let (_, rest) = readme.split_once("<!-- sdc-coverage:begin -->\n").expect("begin marker");
    let (tables, _) = rest.split_once("<!-- sdc-coverage:end -->").expect("end marker");
    let want = vyges_loom::sdc::registry::coverage_markdown();
    if tables != want {
        let diff: Vec<String> = want.lines().zip(tables.lines()).filter(|(w, r)| w != r).take(5).map(|(w, r)| format!("registry: {w}\n  readme: {r}")).collect();
        panic!("README coverage differs from the registry (cargo run --example coverage_table):\n{}", diff.join("\n"));
    }
}

/// The README's usage block names every option `--help` prints, and none it does not: an option
/// added to the tool without the README, or left in the README after it is gone, fails here.
#[test]
fn the_readme_usage_names_every_option_help_prints() {
    let options = |text: &str| -> std::collections::BTreeSet<String> {
        text.split(|c: char| c.is_whitespace() || "[]|,`.:;()".contains(c))
            .filter(|w| w.len() >= 2 && w.starts_with('-') && w[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '-'))
            .filter(|w| w.starts_with("--") || w.len() == 2)
            .map(|w| w.to_string())
            .collect()
    };
    let (code, help, _) = sdc(&["--help"]);
    assert_eq!(code, 0);
    let help = help.split("ENVIRONMENT:").next().expect("help text");
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).expect("README");
    let usage = readme
        .split("<!-- usage:begin -->")
        .nth(1)
        .and_then(|r| r.split("<!-- usage:end -->").next())
        .expect("README usage markers");
    let (h, r) = (options(help), options(usage));
    let missing: Vec<_> = h.difference(&r).collect();
    let extra: Vec<_> = r.difference(&h).collect();
    assert!(missing.is_empty() && extra.is_empty(), "README usage lacks {missing:?}; names {extra:?}, which --help does not");
}

/// SDC 1.4+ `set_data_check -rise` / `-fall` checks one edge of the data. `check` reads it with
/// that meaning and, since no timer here models a one-edge check, warns that it is kept and not
/// applied; it is not an error, and the file is still written back whole.
#[test]
fn check_warns_on_a_one_edge_data_check() {
    let f = scratch("one_edge.sdc");
    std::fs::write(&f, "create_clock -name c -period 10 [get_ports clk]\nset_data_check -rise -from [get_pins r/D] -to [get_pins s/D] 0.1\n").unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--json"]);
    assert_eq!(rc, 0, "{out}{err}");
    let v = json(&out);
    let w: Vec<&serde_json::Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["code"] == "sdc-kept-not-applied").collect();
    assert_eq!(w.len(), 1, "{out}");
    assert_eq!(w[0]["severity"], "warning", "{out}");
    assert_eq!(
        w[0]["message"],
        "`set_data_check -rise`: kept and written back; it can change timing, and no checker or timer here applies it"
    );
    let (rc, out, err) = sdc(&["check", "--sdc", &f, "--fail-on-warning"]);
    assert_eq!(rc, 1, "{out}{err}");
}

/// Only `/` is supported as the hierarchy separator. Any other stops the read (exit 2) with a
/// message naming it, since names written with it would not resolve.
#[test]
fn another_hierarchy_separator_stops_the_read() {
    let f = scratch("separator.sdc");
    std::fs::write(&f, "set_hierarchy_separator .\ncreate_clock -name c -period 10 [get_ports clk]\n").unwrap();
    let (rc, out, err) = sdc(&["check", "--sdc", &f]);
    assert_eq!(rc, 2, "{out}{err}");
    let msg = "`set_hierarchy_separator .`: only `/` is supported, and names using another separator would not resolve; the file is not read";
    assert!(err.contains(msg) || out.contains(msg), "{out}{err}");
    let f = scratch("separator_slash.sdc");
    std::fs::write(&f, "set_hierarchy_separator /\ncreate_clock -name c -period 10 [get_ports clk]\n").unwrap();
    assert_eq!(sdc(&["check", "--sdc", &f]).0, 0);
}

/// A real constraint file (`tests/data/coralnpu/`, Apache-2.0, unmodified), checked without a
/// netlist: the two `if` blocks whose condition is the length of an object query need the
/// design and are an error (not applied); nothing else is an error or a warning (`group_path`
/// is applied by the `sta` timer). `CLK_PERIOD_PS` is removed so the file takes its own period.
#[test]
fn check_reads_a_real_constraint_file() {
    let o = Command::new(env!("CARGO_BIN_EXE_vyges-sdc"))
        .args(["check", "--sdc", "tests/data/coralnpu/rvv_core_mini-asap7.sdc", "--json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("CLK_PERIOD_PS")
        .output()
        .expect("run vyges-sdc");
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(1), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let v = json(&out);
    let codes = |sev: &str| -> Vec<String> {
        v["findings"].as_array().unwrap().iter().filter(|f| f["severity"] == sev).map(|f| f["code"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(codes("error"), ["sdc-not-applied"], "{out}");
    assert!(codes("warning").is_empty(), "{out}");
    assert_eq!((v["errors"].as_u64(), v["warnings"].as_u64(), v["kept_not_applied"].as_u64()), (Some(1), Some(0), Some(0)), "{out}");
}

/// The README's clock-sense section, run: what `check` reports for `set_sense` and
/// `set_clock_sense` (each message as the README quotes it), and what `write` writes back.
#[test]
fn the_readme_claims_about_clock_senses() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    let flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    let check = |name: &str, text: &str| -> Vec<(String, String, String)> {
        let f = scratch(&format!("{name}.sdc"));
        std::fs::write(&f, format!("create_clock -name c -period 10 [get_ports clk]\n{text}\n")).unwrap();
        let (_, out, _) = sdc(&["check", "--sdc", &f, "--json"]);
        let v = json(&out);
        let s = |x: &serde_json::Value| x.as_str().unwrap_or("").to_string();
        v["findings"].as_array().unwrap().iter().map(|f| (s(&f["severity"]), s(&f["code"]), s(&f["message"]))).collect()
    };
    let quoted = |m: &str| assert!(flat.contains(&format!("`` {m}``")) || flat.contains(&format!("``{m}``")) || flat.contains(&format!("`{m}`")), "README does not quote: {m}");

    let f = check("sense_no_type", "set_sense -stop_propagation [get_pins u1/Q]");
    let kept = "`set_sense`: kept and written back; it can change timing, and no checker or timer here applies it";
    let default = "`set_sense` no -type: applied as clock (SDC 2.1 default); the reference timer ignores it without -type";
    assert!(f.contains(&("warning".into(), "sdc-kept-not-applied".into(), kept.into())), "{f:?}");
    assert!(f.contains(&("info".into(), "sdc-sense-type-default".into(), default.into())), "{f:?}");
    quoted(kept);
    quoted(default);

    let f = check("sense_data", "set_sense -type data -positive [get_pins u1/Q]");
    let data = "`set_sense -type data`: not modelled, not applied; the reference timer does not apply it either";
    assert!(f.contains(&("warning".into(), "sdc-not-applied".into(), data.into())), "{f:?}");
    assert!(!f.iter().any(|(s, _, _)| s == "error"), "{f:?}");
    quoted(data);

    // set_clock_sense: kept, not reported as deprecated; -clock is an extension
    let f = check("clock_sense", "set_clock_sense -neg -clock [get_clocks c] [get_pins u1/Q]");
    assert!(f.iter().any(|(s, c, m)| s == "warning" && c == "sdc-kept-not-applied" && m.starts_with("`set_clock_sense`")), "{f:?}");
    assert!(f.iter().any(|(s, c, m)| s == "info" && c == "sdc-extension" && m.starts_with("`set_clock_sense -clock`")), "{f:?}");
    assert!(!f.iter().any(|(_, _, m)| m.contains("deprecated")), "{f:?}");

    // write: no -type is written as -type clock; set_clock_sense as given, options in full;
    // -clocks wins over -clock; -pulse is written back; -type data is not written
    let f = scratch("senses_in.sdc");
    let o = scratch("senses_out.sdc");
    std::fs::write(
        &f,
        "create_clock -name c -period 10 [get_ports clk]\ncreate_clock -name d -period 10 [get_ports clk2]\n\
         set_sense -stop_propagation [get_pins u1/Q]\nset_clock_sense -neg -clock d -clocks c [get_pins u2/Q]\n\
         set_sense -type clock -pulse rise_triggered_high_pulse [get_pins u3/Q]\nset_sense -type data -positive [get_pins u4/Q]\n",
    )
    .unwrap();
    let (rc, out, err) = sdc(&["write", "--sdc", &f, "-o", &o, "--partial", "--json", "-qqq"]);
    assert_eq!(rc, 0, "{out}{err}");
    let w = std::fs::read_to_string(&o).unwrap();
    assert!(w.contains("set_sense -type clock -stop_propagation [get_pins {u1/Q}]"), "{w}");
    assert!(w.contains("set_clock_sense -clocks [get_clocks {c}] -negative [get_pins {u2/Q}]"), "{w}");
    assert!(w.contains("set_sense -type clock -pulse {rise_triggered_high_pulse} [get_pins {u3/Q}]"), "{w}");
    assert!(!w.contains("u4/Q"), "{w}");
    assert!(json(&out)["not_written"].to_string().contains("set_sense"), "{out}");
}

/// The README's design-rule examples: `set_max_fanout -clock_path` (an option no SDC gives it) is
/// an error, and `set_max_capacitance -clock_path` (SDC 1.8 in newer vendor releases) is read,
/// kept and reported as kept but not applied, never refused.
#[test]
fn the_readme_design_rule_examples_hold() {
    let readme = include_str!("../README.md").split_whitespace().collect::<Vec<_>>().join(" ");
    let dir = scratch("design_rules");
    std::fs::create_dir_all(&dir).unwrap();
    let check = |name: &str, line: &str| {
        assert!(readme.contains(&format!("`{line}`")), "the README does not quote `{line}`");
        let f = format!("{dir}/{name}.sdc");
        std::fs::write(&f, format!("create_clock -name c -period 10 [get_ports clk]\n{line}\n")).unwrap();
        let (rc, out, _) = sdc(&["check", "--sdc", f.as_str(), "--json", "-qqq"]);
        (rc, json(&out)["findings"].to_string())
    };
    let (rc, f) = check("fanout", "set_max_fanout -clock_path 8 [get_clocks c]");
    assert_eq!(rc, 1, "{f}");
    assert!(f.contains("-clock_path is not an option of set_max_fanout"), "{f}");
    let (_, f) = check("cap", "set_max_capacitance -clock_path 0.1 [get_clocks c]");
    assert!(f.contains("sdc-kept-not-applied") && f.contains("set_max_capacitance -clock_path"), "{f}");
    assert!(!f.contains("is not an option"), "{f}");
}

/// The README's `set_current_design` claim: read as `current_design`, so the file is read and
/// checked; with a netlist whose top it does not name, `check` warns (`current-design-not-top`).
#[test]
fn set_current_design_is_read_and_warns_on_another_design() {
    assert!(include_str!("../README.md").contains("`current-design-not-top`"), "README");
    let dir = scratch("current_design");
    std::fs::create_dir_all(&dir).unwrap();
    let run = |name: &str| {
        let f = format!("{dir}/{name}.sdc");
        std::fs::write(&f, format!("set_current_design {name}\n{}", std::fs::read_to_string("tests/data/clean.sdc").unwrap())).unwrap();
        let mut a = vec!["check", "--sdc", f.as_str(), "--json", "-qqq"];
        a.extend(DESIGN);
        let (rc, out, err) = sdc(&a);
        (rc, json(&out)["findings"].to_string(), err)
    };
    let (rc, f, err) = run("t");
    assert_eq!(rc, 0, "{f} {err}");
    assert!(!f.contains("current-design-not-top"), "{f}");
    let (rc, f, err) = run("other");
    assert_ne!(rc, 2, "the read stopped: {err}");
    assert!(f.contains("current-design-not-top"), "{f}");
}

const CORAL: &str = "tests/data/coralnpu/rvv_core_mini-asap7.sdc";

fn changes(v: &serde_json::Value) -> Vec<serde_json::Value> {
    v["changes"].as_array().cloned().unwrap_or_default()
}

/// A whole file against itself: equivalent, exit 0.
#[test]
fn diff_of_a_file_against_itself_is_equivalent() {
    let (rc, out, err) = sdc(&["diff", "--sdc", "tests/data/clean.sdc", "--against", "tests/data/clean.sdc", "--json"]);
    assert_eq!(rc, 0, "{err}{out}");
    let v = json(&out);
    assert_eq!(v["equivalent"], true, "{out}");
    assert_eq!(changes(&v).len(), 0);
}

/// The real file against itself is equivalent: its `if` blocks, which need the design to
/// evaluate, are compared by their text.
#[test]
fn diff_compares_unevaluated_blocks_by_their_text() {
    let (rc, out, err) = sdc(&["diff", "--sdc", CORAL, "--against", CORAL, "--json"]);
    assert_eq!(rc, 0, "{err}{out}");
    assert_eq!(json(&out)["equivalent"], true, "{out}");
    // and a changed block body is a change
    let text = std::fs::read_to_string(CORAL).unwrap();
    let body = "  set_case_analysis 0 $te_port\n";
    assert!(text.contains(body), "the test file still has the test-mode case analysis");
    let b = scratch("coral-te-1.sdc");
    std::fs::write(&b, text.replace(body, "  set_case_analysis 1 $te_port\n")).unwrap();
    let (rc, out, err) = sdc(&["diff", "--sdc", CORAL, "--against", &b, "--json"]);
    assert_eq!(rc, 1, "{err}{out}");
    let c = changes(&json(&out));
    assert_eq!(c.len(), 2, "the old block removed, the new one added: {out}");
    assert!(c.iter().all(|c| c["direction"] == "unknown"), "{out}");
}

/// What cannot be compared is said: with a rename map, a raw text cannot be renamed faithfully,
/// so the real file's `if` blocks are not compared, and even against itself it is NOT reported
/// equivalent.
#[test]
fn diff_never_calls_a_file_with_uncompared_commands_equivalent() {
    let m = scratch("noop-map.json");
    std::fs::write(&m, r#"{"no_such_instance": "other"}"#).unwrap();
    let (rc, out, err) = sdc(&["diff", "--sdc", CORAL, "--against", CORAL, "--map", &m, "--json"]);
    assert_eq!(rc, 1, "{err}{out}");
    let v = json(&out);
    assert_eq!(changes(&v).len(), 0, "{out}");
    assert_eq!(v["equivalent"], false);
    assert!(!v["not_compared"]["sdc"].as_array().unwrap().is_empty(), "{out}");
}

/// One false path deleted is ONE change: a removal, and it tightens (the paths it cut are timed
/// again). Exit 1.
#[test]
fn diff_names_a_removed_false_path_as_tightening() {
    let text = std::fs::read_to_string(CORAL).unwrap();
    let line = "set_false_path -from [get_ports -quiet io_aresetn]\n";
    assert!(text.contains(line), "the test file still has the reset false path");
    let b = scratch("coral-no-reset-fp.sdc");
    std::fs::write(&b, text.replace(line, "")).unwrap();
    let (rc, out, err) = sdc(&["diff", "--sdc", CORAL, "--against", &b, "--json"]);
    assert_eq!(rc, 1, "{err}{out}");
    let v = json(&out);
    let c = changes(&v);
    assert_eq!(c.len(), 1, "{out}");
    assert_eq!(c[0]["kind"], "removed");
    assert_eq!(c[0]["command"], "set_false_path");
    assert_eq!(c[0]["direction"], "tightens");
}

/// A longer clock period loosens the clock itself; what the file derives from the period
/// (uncertainty, I/O budgets) changes with it and is reported too.
#[test]
fn diff_of_a_longer_period_loosens_the_clock() {
    let text = std::fs::read_to_string(CORAL).unwrap();
    assert!(text.contains("set clk_period 5000\n"));
    let b = scratch("coral-6000.sdc");
    std::fs::write(&b, text.replace("set clk_period 5000\n", "set clk_period 6000\n")).unwrap();
    let (rc, out, err) = sdc(&["diff", "--sdc", CORAL, "--against", &b, "--json"]);
    assert_eq!(rc, 1, "{err}{out}");
    let v = json(&out);
    let clk: Vec<_> = changes(&v).into_iter().filter(|c| c["command"] == "create_clock").collect();
    assert_eq!(clk.len(), 2, "io_aclk and its virtual clock: {out}");
    assert!(clk.iter().all(|c| c["direction"] == "loosens"), "{out}");
    assert!(changes(&v).iter().any(|c| c["command"] == "set_clock_uncertainty" && c["direction"] == "tightens"), "5% of a longer period is a larger uncertainty: {out}");
}

/// The rename map: an RTL-named file and a netlist-named file are equivalent through the map,
/// and differ without it.
#[test]
fn diff_with_a_rename_map_compares_renamed_objects() {
    let a = scratch("rtl.sdc");
    let b = scratch("net.sdc");
    let m = scratch("names.json");
    std::fs::write(&a, "create_clock -name clk -period 10 [get_ports clk]\nset_false_path -from [get_pins u_cfg/mode_q_reg/Q]\n").unwrap();
    std::fs::write(&b, "create_clock -name clk -period 10 [get_ports clk]\nset_false_path -from [get_pins u42/Q]\n").unwrap();
    std::fs::write(&m, r#"{"u_cfg/mode_q_reg": "u42"}"#).unwrap();
    let (rc, out, err) = sdc(&["diff", "--sdc", &a, "--against", &b, "--map", &m, "--json"]);
    assert_eq!(rc, 0, "{err}{out}");
    let (rc, _, _) = sdc(&["diff", "--sdc", &a, "--against", &b]);
    assert_eq!(rc, 1);
}

#[test]
fn diff_without_both_files_is_a_usage_error() {
    let (rc, _, _) = sdc(&["diff", "--sdc", CORAL]);
    assert_eq!(rc, 2);
    let (rc, out, _) = sdc(&["diff", "--sdc", CORAL, "--against", "tests/data/no-such.sdc", "--json"]);
    assert_eq!(rc, 2);
    assert_eq!(json(&out)["status"], "error");
}
