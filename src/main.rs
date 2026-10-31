//! vyges-sdc — a Loom engine for **SDC constraints**: checking them and writing them.
//!
//! - `check` lints a design's constraints for completeness and consistency against its netlist
//!   (+ Liberty, + optionally the IP's declared clock domains): an unconstrained output, a
//!   clock on a port that drives nothing, an exception that names no real path. A timer
//!   computes timing on whatever constraints it is given; this says whether they are whole.
//! - `write` reads an SDC into the shared model and writes it back out — normalised, in the
//!   order a timer writes its own database. **What is not written is said**: a command the
//!   model does not carry makes `write` refuse (exit 1, every dropped command listed) unless
//!   `--partial` is given, and then the file carries a `# not written:` line for each.
//!
//! Both are the `vyges-loom` implementations (`sdc_lint`, `Sdc::write`), the same code the
//! timing engine runs; this crate is their command line. `--describe` advertises the structured
//! contract to `vyges mcp`; `--json` emits the result envelope.

use serde_json::{json, Value};
use vyges_loom::liberty::{Lib, LibOpts};
use vyges_loom::netlist::Netlist;
use vyges_loom::sdc::{Sdc, SdcDesign};
use vyges_loom::sdc_lint::{LintReport, Severity};
use vyges_loom::sdc_write::WriteOpts;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
vyges loom sdc — SDC constraint checking and writing

USAGE:
  vyges loom sdc check --sdc <f.sdc> --netlist <n.v|n.json> --lib <l.lib> [--lib ...]
                       [--metadata <vyges-metadata.json>] [--fail-on-warning] [--json]
  vyges loom sdc write --sdc <f.sdc> [-o out.sdc] [--netlist <n.v> --lib <l.lib>]
                       [--design NAME] [--partial] [--json]
  vyges loom sdc --describe | --version | --help

COMMANDS:
  check   completeness + consistency of the constraints against the design: I/O with no delay,
          registers no clock reaches, exceptions naming no object or path, endpoint coverage,
          and (with --metadata) the IP's declared clock domains.
  write   the constraints back out as SDC. Refuses — nothing written — when a command would be
          dropped, listing each; --partial writes anyway, with a `# not written:` line per
          dropped command. A netlist (+ Liberty) lets object queries that need the design
          (-filter, -of_objects, -regexp) be evaluated instead of refused.

OPTIONS:
  --sdc FILE            the SDC to read
  --netlist FILE        gate-level Verilog, or Yosys JSON (.json); gzipped is read too
  --lib FILE            Liberty (repeatable; merged in order)
  --metadata FILE       vyges-metadata.json whose clock_domains `check` tests the SDC against
  --fail-on-warning     `check`: exit 1 on a warning too, not only on an error
  -o, --out FILE        `write`: the SDC to write (default stdout; with --json, the envelope's `text`)
  --design NAME         `write`: the current_design name to write
  --partial             `write`: write even when commands are dropped
  --json                emit the JSON result envelope on stdout
  -q, --quiet           raise the event floor to `error` (VYGES_LOG)
  -v, --verbose         lower it to `debug` (adds what was read: clocks, delays, exceptions)
  --describe            print the machine-readable tool contract (for `vyges mcp`)
  --bug-report          file a bug (central: vyges/community)
  --feature-request     request a feature (central)
  --sponsor             sponsor Vyges (github.com/sponsors/vyges-ip)
  --star                star this tool on GitHub

ENVIRONMENT:
  VYGES_LOG             event floor: debug | info | warn | error (an explicit value beats -q/-v)
  VYGES_LOG_FORMAT      events on stderr as `json` (JSONL) or `text`; default text on a TTY, JSONL piped

EVENTS (stderr, vyges-events):
  SDC-READ     debug  what the SDC held: clocks, I/O delays, exceptions, ignored commands
  SDC-<code>   per finding of `check` (e.g. SDC-UNCONSTRAINED-OUTPUT), at its severity
  SDC-CHECK    the `check` summary
  SDC-NOT-WRITTEN  per command `write` left out
  SDC-WRITE    the `write` summary: complete, PARTIAL or REFUSED
  SDC-IPMETA   --metadata could not be read; the other checks still ran
  SDC-INPUT    an input could not be read

EXIT STATUS:
  0  ok        check: no errors (and with --fail-on-warning no warnings); write: written
  1  finding   check found an error (or a warning with --fail-on-warning); write REFUSED
  2  error     usage, or an input that could not be read
";

/// The `--describe` payload the mcp registry parses into typed MCP tools: `sdc` (check) and
/// `sdc.write`.
fn describe() -> Value {
    json!({
        "schema": "vyges-tool-descriptor/1.1",
        "name": "sdc",
        "summary": "SDC constraint check: completeness and consistency of a design's constraints against its netlist and Liberty (unconstrained I/O, clocks reaching no register, exceptions naming no path, endpoint coverage).",
        "maturity": "structured",
        "provenance_limitations": [
            "input_hash covers the argument vector, not the content of the SDC, netlist, Liberty or metadata files it names."
        ],
        "invocation": {
            "args_template": ["check", "--sdc", "{sdc}", "--netlist", "{netlist}", "--lib", "{lib}"],
            "optional": [ { "arg": "metadata", "flag": "--metadata" }, { "arg": "fail_on_warning", "flag": "--fail-on-warning" } ],
            "emits_json": true
        },
        "inputs": {
            "type": "object",
            "required": ["sdc", "netlist", "lib"],
            "properties": {
                "sdc":      { "type": "string", "description": "the SDC constraints to check" },
                "netlist":  { "type": "string", "description": "gate-level Verilog (or Yosys JSON) netlist the constraints apply to" },
                "lib":      { "type": "string", "description": "Liberty library of the netlist's cells" },
                "metadata": { "type": "string", "description": "vyges-metadata.json: its clock_domains are checked against the SDC's clocks" },
                "fail_on_warning": { "type": "boolean", "description": "treat a warning (e.g. an unconstrained output) as a failure, not only an error" }
            }
        },
        "assertion": {
            "id": "sdc-clean",
            "field": "clean",
            "pass_when": { "is_true": true }
        },
        "consumes": ["sdc", "netlist", "liberty"],
        "tools": [
            {
                "name": "sdc.write",
                "summary": "write SDC constraints back out, normalised; refuses (nothing written) when any command would be dropped, listing each, unless partial is set",
                "invocation": {
                    "args_template": ["write", "--sdc", "{sdc}", "-o", "{out}"],
                    "optional": [
                        { "arg": "netlist", "flag": "--netlist" },
                        { "arg": "lib", "flag": "--lib" },
                        { "arg": "design", "flag": "--design" },
                        { "arg": "partial", "flag": "--partial" }
                    ],
                    "emits_json": true
                },
                "inputs": {
                    "type": "object",
                    "required": ["sdc", "out"],
                    "properties": {
                        "sdc":     { "type": "string", "description": "the SDC to read" },
                        "out":     { "type": "string", "description": "path to write the SDC to" },
                        "netlist": { "type": "string", "description": "netlist, to evaluate object queries that need the design" },
                        "lib":     { "type": "string", "description": "Liberty, with netlist, for pin directions in those queries" },
                        "design":  { "type": "string", "description": "the current_design name to write" },
                        "partial": { "type": "boolean", "description": "write even when commands are dropped (each as a `# not written:` line)" }
                    }
                },
                "artifacts": [ { "role": "sdc", "field": "out" } ],
                "assertion": {
                    "id": "sdc-written-whole",
                    "field": "complete",
                    "pass_when": { "is_true": true }
                }
            }
        ]
    })
}

/// The value after `name`, wherever it is.
fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Every value given for a repeatable flag, in order.
fn opts<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].as_str())
        .collect()
}

fn load_netlist(path: &str) -> Result<Netlist, String> {
    let res = if std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        vyges_loom::yosys_json::load(path).map_err(|e| e.to_string())
    } else {
        vyges_loom::netlist::load(path).map_err(|e| e.to_string())
    };
    res.map_err(|e| format!("{path}: {e}"))
}

/// Every library, merged in order (the first one's thresholds rule ports).
fn load_libs(paths: &[&str]) -> Result<Lib, String> {
    let mut lib: Option<Lib> = None;
    for p in paths {
        let one = Lib::load_opts(p, LibOpts::default()).map_err(|e| format!("{p}: {e}"))?;
        match lib.as_mut() {
            None => lib = Some(one),
            Some(first) => first.merge(one),
        }
    }
    Ok(lib.unwrap_or_default())
}

/// Read the SDC; when it holds object queries that need the design and a netlist was given,
/// read it again with the design so they are evaluated rather than refused.
fn load_sdc(path: &str, design: Option<(&Netlist, &Lib)>) -> Result<Sdc, String> {
    let plain = Sdc::load(path).map_err(|e| format!("{path}: {e}"))?;
    match design {
        Some((nl, lib)) if !plain.unevaluated_queries.is_empty() => {
            let d = SdcDesign::from_netlist(nl, Some(lib));
            Sdc::load_with(path, &d).map_err(|e| format!("{path}: {e}"))
        }
        _ => Ok(plain),
    }
}

type Severity_ = vyges_events::Severity;

/// One event on the suite's channel (stderr; `VYGES_LOG` floor, `VYGES_LOG_FORMAT` form). The SDC
/// file is the object every event names, so an agent can key a run's trail on it.
fn event(sev: Severity_, code: &str, msg: String, sdc: &str) {
    vyges_events::emit(
        &vyges_events::Event::new("vyges-sdc", sev, msg)
            .with_code(code)
            .with_objects(vec![format!("sdc:{sdc}")]),
    );
}

/// An input that could not be read: an event, the envelope (with --json), exit 2 — distinct from
/// exit 1, which is a finding about constraints that WERE read.
fn fail(want_json: bool, cmd: &str, sdc: &str, msg: &str) -> i32 {
    event(Severity_::Error, "SDC-INPUT", format!("{cmd}: {msg}"), sdc);
    // The event is the message (error is the top rank, so no floor hides it).
    if want_json {
        println!(
            "{:#}",
            json!({ "schema": "vyges-sdc/1.0", "command": cmd, "status": "error", "error": msg })
        );
    }
    2
}

/// What the reader kept, at debug: the first thing to look at when a check says less than expected.
fn emit_read(sdc: &Sdc, path: &str) {
    event(
        Severity_::Debug,
        "SDC-READ",
        format!(
            "read {path}: {} clock(s), {} input delay(s), {} output delay(s), {} exception(s), {} ignored command(s), {} unevaluated quer(ies)",
            sdc.clocks.len(),
            sdc.input_delays.len(),
            sdc.output_delays.len(),
            sdc.exceptions.len(),
            sdc.ignored.len(),
            sdc.unevaluated_queries.len()
        ),
        path,
    );
}

fn render_check(r: &LintReport) -> String {
    let mut s = String::new();
    // Clean means no errors and no warnings — not an empty report. Coverage is printed on a
    // clean run too: "constrained 100%" is the statement worth having.
    if r.errors() == 0 && r.warnings() == 0 {
        s.push_str("vyges-sdc check — CLEAN ✓  (no constraint issues)\n");
    } else {
        s.push_str(&format!(
            "vyges-sdc check — {} error(s), {} warning(s)\n",
            r.errors(),
            r.warnings()
        ));
    }
    for f in &r.findings {
        s.push_str(&format!("  {:7} [{}] {}\n", f.severity.tag(), f.code, f.message));
    }
    s
}

fn check_json(r: &LintReport, sdc: &str) -> Value {
    let findings: Vec<Value> = r
        .findings
        .iter()
        .map(|f| json!({ "severity": f.severity.tag(), "code": f.code, "message": f.message }))
        .collect();
    json!({
        "schema": "vyges-sdc/1.0",
        "command": "check",
        "status": "ok",
        "sdc": sdc,
        "clean": r.errors() == 0,
        "errors": r.errors(),
        "warnings": r.warnings(),
        "endpoint_coverage_pct": r.endpoint_coverage_pct(),
        "findings": findings
    })
}

fn cmd_check(args: &[String]) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let (Some(sdc_path), Some(nl_path)) = (opt(args, "--sdc"), opt(args, "--netlist")) else {
        eprintln!("vyges-sdc check: --sdc <f.sdc> and --netlist <n.v> are required");
        return 2;
    };
    let lib_paths = opts(args, "--lib");
    if lib_paths.is_empty() {
        eprintln!("vyges-sdc check: --lib <l.lib> is required (it says which cells are registers)");
        return 2;
    }
    let nl = match load_netlist(nl_path) {
        Ok(n) => n,
        Err(e) => return fail(want_json, "check", sdc_path, &e),
    };
    let lib = match load_libs(&lib_paths) {
        Ok(l) if !l.cells.is_empty() => l,
        Ok(_) => return fail(want_json, "check", sdc_path, "no cells in any --lib"),
        Err(e) => return fail(want_json, "check", sdc_path, &e),
    };
    let sdc = match load_sdc(sdc_path, Some((&nl, &lib))) {
        Ok(s) => s,
        Err(e) => return fail(want_json, "check", sdc_path, &e),
    };
    emit_read(&sdc, sdc_path);
    // A metadata file that cannot be read is said, and the other checks still run: an
    // optional input must not switch the whole check off.
    let meta = match opt(args, "--metadata") {
        Some(p) => match vyges_loom::ipmeta::IpMeta::load(p) {
            Ok(m) => Some(m),
            Err(e) => {
                vyges_events::emit(
                    &vyges_events::Event::new(
                        "vyges-sdc",
                        vyges_events::Severity::Warn,
                        format!("{e} — checking without the IP's declared clock domains"),
                    )
                    .with_code("SDC-IPMETA"),
                );
                None
            }
        },
        None => None,
    };
    let r = vyges_loom::sdc_lint::lint_with_meta(&nl, &sdc, &lib, meta.as_ref());
    // One event per finding, keyed by its stable code, so a trail can be clustered without
    // parsing the report.
    for f in &r.findings {
        event(finding_severity(f.severity), &finding_code(f.code), f.message.clone(), sdc_path);
    }
    let sev = if r.errors() > 0 {
        Severity_::Error
    } else if r.warnings() > 0 {
        Severity_::Warn
    } else {
        Severity_::Info
    };
    event(
        sev,
        "SDC-CHECK",
        format!("sdc check on {sdc_path}: {} error(s), {} warning(s)", r.errors(), r.warnings()),
        sdc_path,
    );
    if want_json {
        println!("{:#}", check_json(&r, sdc_path));
    } else {
        print!("{}", render_check(&r));
    }
    let fail_on_warning = args.iter().any(|a| a == "--fail-on-warning");
    i32::from(r.errors() > 0 || (fail_on_warning && r.warnings() > 0))
}

fn finding_severity(s: Severity) -> Severity_ {
    match s {
        Severity::Error => Severity_::Error,
        Severity::Warning => Severity_::Warn,
        Severity::Info => Severity_::Info,
    }
}

/// `unconstrained-output` → `SDC-UNCONSTRAINED-OUTPUT`.
fn finding_code(code: &str) -> String {
    format!("SDC-{}", code.to_ascii_uppercase())
}

fn cmd_write(args: &[String]) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let partial = args.iter().any(|a| a == "--partial");
    let Some(sdc_path) = opt(args, "--sdc") else {
        eprintln!("vyges-sdc write: --sdc <f.sdc> is required");
        return 2;
    };
    let out = opt(args, "-o").or_else(|| opt(args, "--out"));
    let lib_paths = opts(args, "--lib");
    let design = match opt(args, "--netlist") {
        Some(p) => {
            let nl = match load_netlist(p) {
                Ok(n) => n,
                Err(e) => return fail(want_json, "write", sdc_path, &e),
            };
            let lib = match load_libs(&lib_paths) {
                Ok(l) => l,
                Err(e) => return fail(want_json, "write", sdc_path, &e),
            };
            Some((nl, lib))
        }
        None => None,
    };
    let sdc = match load_sdc(sdc_path, design.as_ref().map(|(n, l)| (n, l))) {
        Ok(s) => s,
        Err(e) => return fail(want_json, "write", sdc_path, &e),
    };
    emit_read(&sdc, sdc_path);
    let w = sdc.write(&WriteOpts {
        design: opt(args, "--design").map(str::to_string),
        header: Some(format!("SDC written by vyges-sdc {VERSION}")),
    });
    let complete = w.not_written.is_empty();
    // Refuse rather than ship constraints with a hole in them: with anything dropped and no
    // --partial, nothing is written.
    let refused = !complete && !partial;
    if !refused {
        match out {
            Some(o) => {
                if let Err(e) = std::fs::write(o, &w.text) {
                    return fail(want_json, "write", sdc_path, &format!("{o}: {e}"));
                }
            }
            None if !want_json => print!("{}", w.text),
            None => {}
        }
    }
    let status = if refused { "refused" } else { "ok" };
    // Each dropped command is its own event, at warn — and at error when it made the write refuse.
    let drop_sev = if refused { Severity_::Error } else { Severity_::Warn };
    for n in &w.not_written {
        event(drop_sev, "SDC-NOT-WRITTEN", n.clone(), sdc_path);
    }
    let (sev, msg) = if complete {
        (Severity_::Info, format!("sdc write of {sdc_path}: complete"))
    } else if refused {
        (Severity_::Error, format!("sdc write of {sdc_path}: REFUSED, {} command(s) would be dropped (--partial writes anyway)", w.not_written.len()))
    } else {
        (Severity_::Warn, format!("sdc write of {sdc_path}: PARTIAL, {} command(s) not written", w.not_written.len()))
    };
    event(sev, "SDC-WRITE", msg, sdc_path);
    if want_json {
        let mut r = json!({
            "schema": "vyges-sdc/1.0",
            "command": "write",
            "status": status,
            "sdc": sdc_path,
            "complete": complete,
            "not_written": w.not_written,
        });
        match (out, refused) {
            (Some(o), false) => r["out"] = json!(o),
            (None, false) => r["text"] = json!(w.text),
            _ => {}
        }
        println!("{r:#}");
    }
    i32::from(refused)
}

/// Community links, the same four every shipped engine carries. Bugs and features go to the
/// CENTRAL `vyges/community` tracker, not this repo's issues: one queue for the whole suite.
fn link(flag: &str) -> Option<(&'static str, &'static str)> {
    Some(match flag {
        "--bug-report" => (
            "Report a bug",
            "https://github.com/vyges/community/issues/new?template=bug_report_template.yaml",
        ),
        "--feature-request" => (
            "Request a feature",
            "https://github.com/vyges/community/issues/new?labels=enhancement",
        ),
        "--sponsor" => ("Sponsor Vyges", "https://github.com/sponsors/vyges-ip"),
        "--star" => ("Star this tool", "https://github.com/vyges-tools/sdc"),
        _ => return None,
    })
}

fn run(args: Vec<String>) -> i32 {
    // ⛔ `-q`/`-v` must land in `VYGES_LOG` before the first event: vyges-events reads its floor
    // once. An explicit `VYGES_LOG` wins — the caller who set it meant it.
    if std::env::var_os("VYGES_LOG").is_none() {
        if args.iter().any(|a| a == "-q" || a == "--quiet") {
            std::env::set_var("VYGES_LOG", "error");
        } else if args.iter().any(|a| a == "-v" || a == "--verbose") {
            std::env::set_var("VYGES_LOG", "debug");
        }
    }
    if let Some((what, url)) = args.iter().find_map(|a| link(a)) {
        println!("{what}: {url}");
        return 0;
    }
    match args.first().map(String::as_str) {
        Some("--describe") => {
            println!("{:#}", describe());
            0
        }
        Some("-V") | Some("--version") => {
            // the version AND the commit: a release and a local build of it are different binaries
            println!(
                "vyges-sdc {VERSION} ({})\nCopyright (c) Vyges. Apache-2.0.",
                env!("VYGES_GIT_SHA")
            );
            0
        }
        None | Some("-h") | Some("--help") => {
            print!("{USAGE}");
            0
        }
        Some("check") => cmd_check(&args[1..]),
        Some("write") => cmd_write(&args[1..]),
        Some(other) => {
            eprintln!("vyges-sdc: unknown command '{other}' (try --help)");
            2
        }
    }
}

pub fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names a tool's command line can carry: template tokens and optional args.
    fn passed(t: &Value) -> Vec<String> {
        let inv = &t["invocation"];
        let mut v: Vec<String> = inv["args_template"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a.as_str()?.strip_prefix('{')?.strip_suffix('}').map(str::to_string))
            .collect();
        for o in inv["optional"].as_array().unwrap() {
            v.push(o["arg"].as_str().unwrap().to_string());
        }
        v
    }

    /// Every advertised input reaches the command line and every argument is advertised, for the
    /// primary tool and each sub-tool (the registry's own authoring lint, restated here).
    #[test]
    fn descriptor_inputs_and_args_agree() {
        let d = describe();
        let mut tools = vec![d.clone()];
        tools.extend(d["tools"].as_array().unwrap().iter().cloned());
        for t in &tools {
            let mut have = passed(t);
            have.sort();
            let mut props: Vec<String> = t["inputs"]["properties"].as_object().unwrap().keys().cloned().collect();
            props.sort();
            assert_eq!(have, props, "{}", t["name"]);
            for r in t["inputs"]["required"].as_array().unwrap() {
                assert!(t["invocation"]["args_template"].to_string().contains(&format!("{{{}}}", r.as_str().unwrap())), "{} {r}", t["name"]);
            }
        }
        assert_eq!(d["tools"][0]["name"], "sdc.write");
        assert_eq!(d["tools"][0]["artifacts"][0]["field"], "out");
    }

    /// `maturity` is one of the registry's three levels; anything else does not validate there.
    #[test]
    fn maturity_is_a_legal_level() {
        let m = describe()["maturity"].as_str().unwrap().to_string();
        assert!(["discovered", "structured", "workflow-validated"].contains(&m.as_str()), "{m}");
        assert_eq!(describe()["schema"], "vyges-tool-descriptor/1.1");
    }

    #[test]
    fn repeatable_lib_keeps_every_value_in_order() {
        let a: Vec<String> = ["--lib", "a.lib", "--sdc", "x", "--lib", "b.lib"].iter().map(|s| s.to_string()).collect();
        assert_eq!(opts(&a, "--lib"), vec!["a.lib", "b.lib"]);
        assert_eq!(opt(&a, "--sdc"), Some("x"));
    }
}
