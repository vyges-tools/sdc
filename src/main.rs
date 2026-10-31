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

usage:
  vyges loom sdc check --sdc <f.sdc> --netlist <n.v|n.json> --lib <l.lib> [--lib ...] [--metadata <vyges-metadata.json>] [--json]
  vyges loom sdc write --sdc <f.sdc> [-o out.sdc] [--netlist <n.v> --lib <l.lib>] [--design NAME] [--partial] [--json]
  vyges loom sdc --describe        structured tool contract (for `vyges mcp`)
  vyges loom sdc --version | --help

check   completeness + consistency of the constraints against the design; exit 1 on any error.
write   the constraints back out as SDC. Refuses (exit 1, nothing written) when a command
        would be dropped, listing each; --partial writes anyway, with a `# not written:` line
        per dropped command. A netlist (+ Liberty) lets object queries that need the design
        (-filter, -of_objects, -regexp) be evaluated instead of refused.
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
            "optional": [ { "arg": "metadata", "flag": "--metadata" } ],
            "emits_json": true
        },
        "inputs": {
            "type": "object",
            "required": ["sdc", "netlist", "lib"],
            "properties": {
                "sdc":      { "type": "string", "description": "the SDC constraints to check" },
                "netlist":  { "type": "string", "description": "gate-level Verilog (or Yosys JSON) netlist the constraints apply to" },
                "lib":      { "type": "string", "description": "Liberty library of the netlist's cells" },
                "metadata": { "type": "string", "description": "vyges-metadata.json: its clock_domains are checked against the SDC's clocks" }
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

fn fail(want_json: bool, cmd: &str, msg: &str) -> i32 {
    if want_json {
        println!(
            "{:#}",
            json!({ "schema": "vyges-sdc/1.0", "command": cmd, "status": "error", "error": msg })
        );
    } else {
        eprintln!("vyges-sdc {cmd}: {msg}");
    }
    1
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
        Err(e) => return fail(want_json, "check", &e),
    };
    let lib = match load_libs(&lib_paths) {
        Ok(l) if !l.cells.is_empty() => l,
        Ok(_) => return fail(want_json, "check", "no cells in any --lib"),
        Err(e) => return fail(want_json, "check", &e),
    };
    let sdc = match load_sdc(sdc_path, Some((&nl, &lib))) {
        Ok(s) => s,
        Err(e) => return fail(want_json, "check", &e),
    };
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
    let sev = if r.errors() > 0 {
        vyges_events::Severity::Error
    } else if r.warnings() > 0 {
        vyges_events::Severity::Warn
    } else {
        vyges_events::Severity::Info
    };
    vyges_events::emit(
        &vyges_events::Event::new(
            "vyges-sdc",
            sev,
            format!("sdc check on {sdc_path}: {} error(s), {} warning(s)", r.errors(), r.warnings()),
        )
        .with_code("SDC-CHECK")
        .with_objects(vec![format!("sdc:{sdc_path}")]),
    );
    if want_json {
        println!("{:#}", check_json(&r, sdc_path));
    } else {
        print!("{}", render_check(&r));
    }
    i32::from(r.findings.iter().any(|f| f.severity == Severity::Error))
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
                Err(e) => return fail(want_json, "write", &e),
            };
            let lib = match load_libs(&lib_paths) {
                Ok(l) => l,
                Err(e) => return fail(want_json, "write", &e),
            };
            Some((nl, lib))
        }
        None => None,
    };
    let sdc = match load_sdc(sdc_path, design.as_ref().map(|(n, l)| (n, l))) {
        Ok(s) => s,
        Err(e) => return fail(want_json, "write", &e),
    };
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
                    return fail(want_json, "write", &format!("{o}: {e}"));
                }
            }
            None if !want_json => print!("{}", w.text),
            None => {}
        }
    }
    let status = if refused { "refused" } else { "ok" };
    vyges_events::emit(
        &vyges_events::Event::new(
            "vyges-sdc",
            if complete { vyges_events::Severity::Info } else if refused { vyges_events::Severity::Error } else { vyges_events::Severity::Warn },
            if complete {
                format!("sdc write of {sdc_path}: complete")
            } else if refused {
                format!("sdc write of {sdc_path}: REFUSED, {} command(s) would be dropped (--partial writes anyway)", w.not_written.len())
            } else {
                format!("sdc write of {sdc_path}: PARTIAL, {} command(s) not written", w.not_written.len())
            },
        )
        .with_code("SDC-WRITE")
        .with_objects(vec![format!("sdc:{sdc_path}")]),
    );
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
    } else if !complete {
        eprintln!(
            "vyges-sdc write: {} {} command(s) not written:",
            if refused { "REFUSED —" } else { "PARTIAL —" },
            w.not_written.len()
        );
        for n in &w.not_written {
            eprintln!("  {n}");
        }
        if refused {
            eprintln!("  (nothing written; --partial writes the rest, with a `# not written:` line for each)");
        }
    }
    i32::from(refused)
}

fn run(args: Vec<String>) -> i32 {
    match args.first().map(String::as_str) {
        Some("--describe") => {
            println!("{:#}", describe());
            0
        }
        Some("-V") | Some("--version") => {
            println!("vyges-sdc {VERSION}");
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
