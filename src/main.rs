//! vyges-sdc — a Loom engine for **SDC constraints**: checking them and writing them.
//!
//! - `check` lints a design's constraints for completeness and consistency against its netlist
//!   (+ Liberty, + optionally the IP's declared clock domains): an unconstrained output, a
//!   clock on a port that drives nothing, an exception that names no real path. A timer
//!   computes timing on whatever constraints it is given; this says whether they are whole.
//!   Without a netlist it checks what the file alone can say: every command it did not apply
//!   (an error when the command affects timing), clock periods, duplicate names.
//! - `write` reads an SDC into the shared model and writes it back out — normalised, in the
//!   order a timer writes its own database. **What is not written is said**: a command the
//!   model does not carry makes `write` refuse (exit 1, every dropped command listed) unless
//!   `--partial` is given, and then the file carries a `# not written:` line for each.
//!
//! - `diff` compares two SDC files as CONSTRAINTS, not text: each constraint added, removed or
//!   changed, and whether the change loosens or tightens what is checked. A rename map lets an
//!   RTL file be compared with a netlist file.
//!
//! - `map` resolves an RTL constraint file's objects against the ELABORATED design (ports,
//!   registers named by their RTL names, nets) and carries each to an implementation netlist
//!   through a proved register map (`vyges physical lec map --write-map`); what names nothing,
//!   and what is not carried, is reported with why.
//!
//! All are the `vyges-loom` implementations (`sdc_lint`, `Sdc::write`), the same code the
//! timing engine runs; this crate is their command line. `--describe` advertises the structured
//! contract to `vyges mcp`; `--json` emits the result envelope.

use serde_json::{json, Value};
use vyges_loom::liberty::{Lib, LibOpts};
use vyges_loom::netlist::Netlist;
use vyges_loom::sdc::{ReadOpts, Sdc, SdcDesign};
use vyges_loom::sdc_lint::{Finding, LintReport, Severity};
use vyges_loom::sdc_waivers::{self, Waiver};
use vyges_loom::sdc_diff::{Change, DiffOpts, SdcDiff};
use vyges_loom::sdc_write::WriteOpts;
use vyges_loom::verbosity;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
vyges physical sdc — SDC constraint checking and writing

USAGE:
  vyges physical sdc check --sdc <f.sdc> [--netlist <n.v|n.json> --lib <l.lib> [--lib ...]]
                       [--top NAME] [--keep MODULE ...] [--metadata <vyges-metadata.json>]
                       [--define NAME=VALUE ...] [--waivers <w.tcl>] [--write-waivers <w.tcl>]
                       [--fail-on-warning] [--json] [-q|-v ...]
  vyges physical sdc write --sdc <f.sdc> [-o out.sdc] [--netlist <n.v> --lib <l.lib> [--lib ...]]
                       [--top NAME] [--keep MODULE ...] [--define NAME=VALUE ...]
                       [--design NAME] [--partial] [--json] [-q|-v ...]
  vyges physical sdc diff --sdc <a.sdc> --against <b.sdc> [--map <names.json>]
                       [--netlist <n.v> --lib <l.lib> [--lib ...]] [--top NAME] [--keep MODULE ...]
                       [--define NAME=VALUE ...] [--json] [-q|-v ...]
  vyges physical sdc map --sdc <rtl.sdc> --netlist <elab.v> --lib <l.lib> [--lib ...]
                       --map <names.json> [--ranges <ranges.json>] [--revised <impl.v>]
                       [--top NAME] [--keep MODULE ...]
                       [--define NAME=VALUE ...] [--json]
  vyges physical sdc waivers export --waivers <w.tcl> --template <t.txt> [-o out.tcl] [--json]
  vyges physical sdc --describe | --version | --help
  vyges physical sdc --bug-report | --feature-request | --sponsor | --star

COMMANDS:
  check   completeness + consistency of the constraints against the design: I/O with no delay,
          registers no clock reaches, exceptions naming no object or path, endpoint coverage,
          and (with --metadata) the IP's declared clock domains. Without --netlist: the checks
          the file alone supports, and every command not applied (an error when it affects
          timing: a constraint the timer will not see is a hole, not a detail).
  write   the constraints back out as SDC. Refuses — nothing written — when a command would be
          dropped, listing each; --partial writes anyway, with a `# not written:` line per
          dropped command. A netlist (+ Liberty) lets object queries that need the design
          (-filter, -of_objects, -regexp) be evaluated instead of refused.
  diff    two SDC files compared as constraints: added, removed and changed, each marked as
          loosening, tightening, neutral (a -comment) or unknown. Order, name order in a list,
          one command on several ports against one per port, and a value a later command
          overrides are not differences. Objects are compared as written; --netlist reads both
          files against the design, so the objects queries resolve to are compared instead.
          A command the model does not carry is compared by its text, variables substituted.
  map     an RTL constraint file's objects resolved against the elaborated netlist (registers
          by the RTL name on their stored-value output net) and carried to the implementation:
          a port or a clock as itself, a register or its pin through --map (the register map
          `vyges physical lec map` proves and writes). A pattern that names nothing, a register
          with no proved pair, and a net or combinational cell (no proved match exists for one)
          are reported, each with why, as is a command whose objects could not be read.
          --revised checks each carried name exists there.
  waivers export  a waiver file (`waive_finding` commands) written as another tool's Tcl waiver
          commands, by a template that maps each finding code to that tool's rule ids. A
          waiver whose code the template does not map is listed and not written.

OPTIONS:
  --sdc FILE            the SDC to read
  --netlist FILE        gate-level Verilog, or Yosys JSON (.json); gzipped is read too. A
                        hierarchical netlist is flattened, as the timer reads it
  --top NAME            the netlist's top module, when the file has more than one candidate
  --keep MODULE         a module kept as a block, not flattened (repeatable)
  --lib FILE            Liberty (repeatable; merged in order)
  --metadata FILE       vyges-metadata.json whose clock_domains `check` tests the SDC against
  --define NAME=VALUE   a Tcl variable set before the SDC is read (repeatable): what a flow's
                        setup script defines and its constraint files only use
  --fail-on-warning     `check`: exit 1 on a warning too, not only on an error
  --waivers FILE        `check`: a Tcl file of `waive_finding` commands; a finding it waives is
                        listed as waived and counts as neither error nor warning. A waiver that
                        matches nothing, has no -reason or is past -expires is a warning itself
  --write-waivers FILE  `check`: write a `waive_finding` template for every error and warning
                        still active, each with an empty -reason to fill in (never overwrites)
  -o, --out FILE        `write`: the SDC to write; `waivers export`: the waivers to write, never
                        over a file (default stdout; with --json, the envelope's `text`)
  --design NAME         `write`: the current_design name to write
  --partial             `write`: write even when commands are dropped
  --against FILE        `diff`: the SDC to compare --sdc with (the newer, or the netlist one)
  --template FILE       `waivers export`: the export template (command line, object form, rule map)
  --map FILE            `diff`: JSON object {\"old name\": \"new name\", ...}, applied to the object
                        names of the --sdc file (a name, or the instance part of a pin) before comparing
  --revised FILE        `map`: the implementation netlist the objects are carried to
  --ranges FILE         `map`: each vector's declared range (`vyges physical syn sv-ranges`);
                        elaboration numbers bits from 0, so an RTL index (`r[8]` of [11:8]) is
                        translated to the elaborated one (`r[0]`), registers and ports alike, and
                        an index outside the declared range is reported
  --json                emit the JSON result envelope on stdout
  -q, --quiet           fewer events (repeatable): -q warn, -qq error, -qqq none
  -v, --verbose         more (repeatable): -v debug (adds what was read), -vv trace
  --describe            print the machine-readable tool contract (for `vyges mcp`)
  --bug-report          file a bug (central: vyges/community)
  --feature-request     request a feature (central)
  --sponsor             sponsor Vyges (github.com/sponsors/vyges-ip)
  --star                star this tool on GitHub

ENVIRONMENT:
  VYGES_LOG             level when no -q/-v is given: off|error|warn|info|debug|trace or 0-5
                        (inherited from `vyges`, so it sets the whole command tree)
  VYGES_LOG_FORMAT      events on stderr as `json` (JSONL) or `text`; default text on a TTY, JSONL piped

EVENTS (stderr, vyges-events):
  SDC-READ     debug  what the SDC held: clocks, I/O delays, exceptions, ignored commands
  SDC-<code>   per finding of `check` (e.g. SDC-UNCONSTRAINED-OUTPUT), at its severity
  SDC-CHECK    the `check` summary
  SDC-NOT-WRITTEN  per command `write` left out (for `map`, per command whose objects were not read)
  SDC-WRITE    the `write` summary: complete, PARTIAL or REFUSED
  SDC-DIFF-CHANGE  per change `diff` found (info; warn when it loosens or is unknown)
  SDC-DIFF     the `diff` summary: equivalent, or the changes by direction
  SDC-MAP-BIT  per RTL bit index `map` translated by its declared range (info)
  SDC-MAP-UNKNOWN  per pattern `map` found naming nothing in the elaborated design (or an index
               outside its declared range)
  SDC-MAP-UNCARRIED  per object `map` could not carry to the implementation, with why
  SDC-MAP      the `map` summary
  SDC-WAIVER-SKIPPED  per waiver `waivers export` did not write (unmapped code, no reason, malformed)
  SDC-WAIVERS-EXPORT  the `waivers export` summary
  SDC-IPMETA   --metadata could not be read; the other checks still ran
  SDC-FLATTEN  a hierarchical netlist was flattened, and anything it found (undefined cells)
  SDC-INPUT    an input could not be read

EXIT STATUS:
  0  ok        check: no errors (and with --fail-on-warning no warnings); write: written;
               diff: equivalent; map: every object resolved and carried;
               waivers export: every waiver written
  1  finding   check found an error (or a warning with --fail-on-warning); write REFUSED;
               diff: the files differ, or a command in either could not be compared;
               map: an object names nothing, is not carried, or cannot be resolved;
               waivers export: a waiver was not written
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
            "args_template": ["check", "--sdc", "{sdc}"],
            "optional": [
                { "arg": "netlist", "flag": "--netlist" },
                { "arg": "lib", "flag": "--lib" },
                { "arg": "metadata", "flag": "--metadata" },
                { "arg": "fail_on_warning", "flag": "--fail-on-warning" },
                { "arg": "waivers", "flag": "--waivers" }
            ],
            "emits_json": true
        },
        "inputs": {
            "type": "object",
            "required": ["sdc"],
            "properties": {
                "sdc":      { "type": "string", "description": "the SDC constraints to check" },
                "netlist":  { "type": "string", "description": "gate-level Verilog (or Yosys JSON) netlist the constraints apply to; without it only the checks the file alone supports run" },
                "lib":      { "type": "string", "description": "Liberty library of the netlist's cells (required with netlist)" },
                "metadata": { "type": "string", "description": "vyges-metadata.json: its clock_domains are checked against the SDC's clocks" },
                "fail_on_warning": { "type": "boolean", "description": "treat a warning (e.g. an unconstrained output) as a failure, not only an error" },
                "waivers":  { "type": "string", "description": "a Tcl file of waive_finding commands: the findings it waives are listed as waived and not counted" }
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
            },
            {
                "name": "sdc.diff",
                "summary": "compare two SDC files as constraints, not text: each constraint added, removed or changed, and whether it loosens or tightens what is checked",
                "invocation": {
                    "args_template": ["diff", "--sdc", "{sdc}", "--against", "{against}"],
                    "optional": [
                        { "arg": "map", "flag": "--map" },
                        { "arg": "netlist", "flag": "--netlist" },
                        { "arg": "lib", "flag": "--lib" }
                    ],
                    "emits_json": true
                },
                "inputs": {
                    "type": "object",
                    "required": ["sdc", "against"],
                    "properties": {
                        "sdc":     { "type": "string", "description": "the first SDC (the older, or the RTL one)" },
                        "against": { "type": "string", "description": "the SDC to compare it with" },
                        "map":     { "type": "string", "description": "JSON object of old name to new name, applied to the first file's objects" },
                        "netlist": { "type": "string", "description": "netlist both files are read against, so queries are compared by what they resolve to" },
                        "lib":     { "type": "string", "description": "Liberty, with netlist" }
                    }
                },
                "assertion": {
                    "id": "sdc-equivalent",
                    "field": "equivalent",
                    "pass_when": { "is_true": true }
                }
            },
            {
                "name": "sdc.map",
                "summary": "resolve an RTL constraint file's objects against the elaborated netlist and carry each to the implementation through a proved register map; reports what names nothing and what is not carried, with why",
                "invocation": {
                    "args_template": ["map", "--sdc", "{sdc}", "--netlist", "{netlist}", "--lib", "{lib}", "--map", "{map}"],
                    "optional": [
                        { "arg": "revised", "flag": "--revised" },
                        { "arg": "ranges", "flag": "--ranges" }
                    ],
                    "emits_json": true
                },
                "inputs": {
                    "type": "object",
                    "required": ["sdc", "netlist", "lib", "map"],
                    "properties": {
                        "sdc":     { "type": "string", "description": "the RTL constraint file, written against the elaborated names" },
                        "netlist": { "type": "string", "description": "the elaborated netlist (registers named by the RTL name on their output net)" },
                        "lib":     { "type": "string", "description": "Liberty of the netlist's cells (registers are its ff cells)" },
                        "map":     { "type": "string", "description": "JSON register map, elaborated name to implementation name (lec map --write-map)" },
                        "revised": { "type": "string", "description": "the implementation netlist; each carried name is checked to exist there" },
                        "ranges":  { "type": "string", "description": "declared vector ranges (syn sv-ranges): RTL bit indices are translated to the elaborated ones" }
                    }
                },
                "assertion": {
                    "id": "sdc-objects-carried",
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

/// The netlist as one flat top: a hierarchical Verilog netlist is flattened (`--top` names the
/// top when the file has several; `--keep` modules stay blocks), as the timer reads it, so a
/// constraint on `u_core/u_alu/…` names an object that exists. Flattening a flat netlist
/// changes nothing. What flattening found (cells no library defines, kept names the file lacks)
/// is an event.
fn load_netlist(path: &str, args: &[String], lib: &Lib, sdc: &str) -> Result<Netlist, String> {
    if std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        return vyges_loom::yosys_json::load(path).map_err(|e| format!("{path}: {e}"));
    }
    let d = vyges_loom::netlist::load_compact_opt(path, opt(args, "--top")).map_err(|e| e.to_string())?;
    let fo = vyges_loom::netlist::FlattenOptions {
        keep: opts(args, "--keep").iter().map(|k| k.to_string()).collect(),
        library_cells: (!lib.cells.is_empty()).then(|| lib.cells.keys().cloned().collect()),
    };
    let (flat, report) = d.flatten(&fo);
    if report.expanded > 0 {
        event(
            Severity_::Info,
            "SDC-FLATTEN",
            format!("{path}: top {} flattened: {} module instance(s) expanded, {} leaf instance(s)", report.top, report.expanded, report.leaves),
            sdc,
        );
    }
    if let Some(h) = report.health() {
        event(Severity_::Warn, "SDC-FLATTEN", format!("{path}: {h}"), sdc);
    }
    Ok(flat.top_netlist())
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

/// How the SDC is read: `--define NAME=VALUE` variables, set in order before it, and its own
/// directory for a relative `source`.
fn read_opts(args: &[String], path: &str) -> Result<ReadOpts, String> {
    let mut defines = Vec::new();
    for d in opts(args, "--define") {
        let Some((n, v)) = d.split_once('=') else {
            return Err(format!("--define {d}: expected NAME=VALUE"));
        };
        defines.push((n.trim().to_string(), v.to_string()));
    }
    let base_dir = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
    Ok(ReadOpts { defines, base_dir })
}

/// Read the SDC; when a netlist was given, against the design, as the reference reads it: object
/// queries are evaluated rather than refused, and what the reference looks up in the design or its
/// libraries (a `-reference_pin`, an operating condition, a wire load model) is checked there,
/// and a generated clock whose master only the design gives takes its waveform from it.
fn load_sdc(path: &str, design: Option<(&Netlist, &Lib)>, ro: &ReadOpts) -> Result<Sdc, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    match design {
        Some((nl, lib)) => {
            let d = SdcDesign::from_netlist(nl, Some(lib));
            let mut sdc = Sdc::parse_with_opts(&text, &d, ro).map_err(|e| format!("{path}: {e}"))?;
            // after the read, as the reference does: each generated clock's waveform from the
            // master the design gives it (a -source with no clock defined on it)
            vyges_loom::sdc_genclk::update_generated_clks(&mut sdc, nl, lib);
            Ok(sdc)
        }
        None => Sdc::parse_opts(&text, ro).map_err(|e| format!("{path}: {e}")),
    }
}

type Severity_ = vyges_events::Severity;

/// One event on the suite's channel (stderr; `VYGES_LOG` floor, `VYGES_LOG_FORMAT` form). The SDC
/// file is the object every event names, so an agent can key a run's trail on it.
fn event(sev: Severity_, code: &str, msg: String, sdc: &str) {
    // The suite's level (`-q`/`-v`/`VYGES_LOG`, resolved in `run`) decides; vyges-events is told
    // to pass everything, because its own floor cannot say `off`.
    let level = match sev {
        Severity_::Trace => verbosity::Level::Trace,
        Severity_::Debug => verbosity::Level::Debug,
        Severity_::Info => verbosity::Level::Info,
        Severity_::Warn => verbosity::Level::Warn,
        Severity_::Error => verbosity::Level::Error,
    };
    if !verbosity::enabled(level) {
        return;
    }
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

fn render_check(r: &LintReport, waived: &[(Finding, Waiver)]) -> String {
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
    for (f, w) in waived {
        s.push_str(&format!("  {:7} [{}] {} (waived: {})\n", "waived", f.code, f.message, w.reason));
    }
    s
}

fn check_json(r: &LintReport, waived: &[(Finding, Waiver)], sdc: &str) -> Value {
    let findings: Vec<Value> = r
        .findings
        .iter()
        .map(|f| json!({ "id": sdc_waivers::finding_id(f), "severity": f.severity.tag(), "code": f.code, "message": f.message }))
        .collect();
    let waived: Vec<Value> = waived
        .iter()
        .map(|(f, w)| {
            json!({ "id": w.id, "severity": f.severity.tag(), "code": f.code, "message": f.message,
                    "reason": w.reason, "owner": w.owner, "expires": w.expires })
        })
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
        // kinds of command read and written back that no checker or timer here applies (each one
        // is a finding too): `clean` is about errors, and this is what it does not cover
        "kept_not_applied": r.findings.iter().filter(|f| f.code == "sdc-kept-not-applied").count(),
        "findings": findings,
        // findings a --waivers file waived: not counted in errors or warnings, listed with why
        "waived": waived
    })
}

fn cmd_check(args: &[String]) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let Some(sdc_path) = opt(args, "--sdc") else {
        eprintln!("vyges-sdc check: --sdc <f.sdc> is required");
        return 2;
    };
    let ro = match read_opts(args, sdc_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vyges-sdc check: {e}");
            return 2;
        }
    };
    // Without a design: what the file alone can say, and every command it did not apply.
    let Some(nl_path) = opt(args, "--netlist") else {
        let sdc = match load_sdc(sdc_path, None, &ro) {
            Ok(s) => s,
            Err(e) => return fail(want_json, "check", sdc_path, &e),
        };
        emit_read(&sdc, sdc_path);
        return report_check(args, &vyges_loom::sdc_lint::lint_without_design(&sdc), sdc_path);
    };
    let lib_paths = opts(args, "--lib");
    if lib_paths.is_empty() {
        eprintln!("vyges-sdc check: --lib <l.lib> is required (it says which cells are registers)");
        return 2;
    }
    let lib = match load_libs(&lib_paths) {
        Ok(l) if !l.cells.is_empty() => l,
        Ok(_) => return fail(want_json, "check", sdc_path, "no cells in any --lib"),
        Err(e) => return fail(want_json, "check", sdc_path, &e),
    };
    let nl = match load_netlist(nl_path, args, &lib, sdc_path) {
        Ok(n) => n,
        Err(e) => return fail(want_json, "check", sdc_path, &e),
    };
    let sdc = match load_sdc(sdc_path, Some((&nl, &lib)), &ro) {
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
    report_check(args, &r, sdc_path)
}

/// A check's waivers applied (`--waivers`), its waiver template written (`--write-waivers`), its
/// events, its report (text or `--json`), and its exit status.
fn report_check(args: &[String], all: &LintReport, sdc_path: &str) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let today = sdc_waivers::utc_today();
    let (active, waived) = match opt(args, "--waivers") {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => {
                let a = sdc_waivers::apply(all, &sdc_waivers::parse(&t), &today);
                (a.active, a.waived)
            }
            Err(e) => return fail(want_json, "check", sdc_path, &format!("--waivers {p}: {e}")),
        },
        None => (LintReport { findings: all.findings.clone() }, Vec::new()),
    };
    let r = &active;
    if let Some(p) = opt(args, "--write-waivers") {
        // the findings still active, less those about the waiver file itself
        let todo = LintReport { findings: r.findings.iter().filter(|f| !f.code.starts_with("waiver-")).cloned().collect() };
        let header = format!("Waivers for {sdc_path}, written by vyges-sdc {VERSION} on {today}.");
        let text = sdc_waivers::write_template(&todo, &header);
        // never over another file: a waiver file holds reasons someone wrote
        let written = std::fs::OpenOptions::new().write(true).create_new(true).open(p).and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
        if let Err(e) = written {
            return fail(want_json, "check", sdc_path, &format!("--write-waivers {p}: {e} (it is never overwritten; name a new file)"));
        }
    }
    for (f, w) in &waived {
        event(Severity_::Info, &finding_code(f.code), format!("waived ({}): {} — {}", w.id, f.message, w.reason), sdc_path);
    }
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
        format!("sdc check on {sdc_path}: {} error(s), {} warning(s), {} waived", r.errors(), r.warnings(), waived.len()),
        sdc_path,
    );
    if want_json {
        println!("{:#}", check_json(r, &waived, sdc_path));
    } else {
        print!("{}", render_check(r, &waived));
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
    let ro = match read_opts(args, sdc_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vyges-sdc write: {e}");
            return 2;
        }
    };
    let lib_paths = opts(args, "--lib");
    let design = match opt(args, "--netlist") {
        Some(p) => {
            let lib = match load_libs(&lib_paths) {
                Ok(l) => l,
                Err(e) => return fail(want_json, "write", sdc_path, &e),
            };
            let nl = match load_netlist(p, args, &lib, sdc_path) {
                Ok(n) => n,
                Err(e) => return fail(want_json, "write", sdc_path, &e),
            };
            Some((nl, lib))
        }
        None => None,
    };
    let sdc = match load_sdc(sdc_path, design.as_ref().map(|(n, l)| (n, l)), &ro) {
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

/// The rename map: a JSON object of old name to new name. Longest old name first, so a pin's
/// instance prefix never shadows a longer one.
fn load_map(path: &str) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let Some(obj) = v.as_object() else {
        return Err(format!("{path}: expected a JSON object of old name to new name"));
    };
    let mut out = Vec::new();
    for (k, v) in obj {
        let Some(to) = v.as_str() else {
            return Err(format!("{path}: {k}: the new name must be a string"));
        };
        out.push((k.clone(), to.to_string()));
    }
    out.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    Ok(out)
}

/// Declared vector ranges (`vyges physical syn sv-ranges` writes them): `{"ranges": {name:
/// [left, right]}}`, or that object alone.
fn load_ranges(path: &str) -> Result<vyges_loom::sdc_rtl_map::DeclaredRanges, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let obj = v.get("ranges").unwrap_or(&v).as_object().ok_or_else(|| format!("{path}: expected a JSON object of name to [left, right]"))?;
    let mut out = vyges_loom::sdc_rtl_map::DeclaredRanges::new();
    for (k, r) in obj {
        match r.as_array().map(|a| a.iter().map(Value::as_i64).collect::<Vec<_>>()).as_deref() {
            Some([Some(l), Some(rt)]) => {
                out.insert(k.clone(), (*l, *rt));
            }
            _ => return Err(format!("{path}: {k}: expected [left, right], integers")),
        }
    }
    Ok(out)
}

/// One change as a line: `changed  loosens  create_clock -name clk  -period 10 -> 12`.
fn render_change(c: &Change) -> String {
    let what = match c.kind.as_str() {
        "changed" => c
            .fields
            .iter()
            .map(|f| format!("{} {} -> {}", f.field, f.a.as_deref().unwrap_or("(none)"), f.b.as_deref().unwrap_or("(none)")))
            .collect::<Vec<_>>()
            .join(", "),
        "added" => c.b.clone().unwrap_or_default(),
        _ => c.a.clone().unwrap_or_default(),
    };
    format!("{:<8} {:<9} {}  {}", c.kind.as_str(), c.direction.as_str(), c.key, what)
}

fn change_json(c: &Change) -> Value {
    json!({
        "kind": c.kind.as_str(),
        "direction": c.direction.as_str(),
        "command": c.command,
        "key": c.key,
        "fields": c.fields.iter().map(|f| json!({ "field": f.field, "a": f.a, "b": f.b })).collect::<Vec<_>>(),
        "a": c.a,
        "b": c.b,
    })
}

/// Changes counted by direction, in a fixed order.
fn by_direction(d: &SdcDiff) -> Value {
    let n = |name: &str| d.changes.iter().filter(|c| c.direction.as_str() == name).count();
    json!({ "loosens": n("loosens"), "tightens": n("tightens"), "neutral": n("neutral"), "unknown": n("unknown") })
}

fn cmd_diff(args: &[String]) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let (Some(a_path), Some(b_path)) = (opt(args, "--sdc"), opt(args, "--against")) else {
        eprintln!("vyges-sdc diff: --sdc <a.sdc> and --against <b.sdc> are required");
        return 2;
    };
    let rename = match opt(args, "--map") {
        Some(m) => match load_map(m) {
            Ok(r) => r,
            Err(e) => return fail(want_json, "diff", a_path, &e),
        },
        None => Vec::new(),
    };
    let lib_paths = opts(args, "--lib");
    let design = match opt(args, "--netlist") {
        Some(p) => {
            let lib = match load_libs(&lib_paths) {
                Ok(l) => l,
                Err(e) => return fail(want_json, "diff", a_path, &e),
            };
            let nl = match load_netlist(p, args, &lib, a_path) {
                Ok(n) => n,
                Err(e) => return fail(want_json, "diff", a_path, &e),
            };
            Some((nl, lib))
        }
        None => None,
    };
    let mut sides = Vec::new();
    for path in [a_path, b_path] {
        let ro = match read_opts(args, path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("vyges-sdc diff: {e}");
                return 2;
            }
        };
        match load_sdc(path, design.as_ref().map(|(n, l)| (n, l)), &ro) {
            Ok(s) => {
                emit_read(&s, path);
                sides.push(s);
            }
            Err(e) => return fail(want_json, "diff", path, &e),
        }
    }
    let d = vyges_loom::sdc_diff::diff(&sides[0], &sides[1], &DiffOpts { rename });
    let equivalent = d.equivalent();
    for c in &d.changes {
        let sev = match c.direction.as_str() {
            "loosens" | "unknown" => Severity_::Warn,
            _ => Severity_::Info,
        };
        event(sev, "SDC-DIFF-CHANGE", render_change(c), b_path);
    }
    for (side, list) in [(a_path, &d.not_compared[0]), (b_path, &d.not_compared[1])] {
        for n in list {
            event(Severity_::Warn, "SDC-NOT-WRITTEN", format!("{side}: not compared: {n}"), side);
        }
    }
    if d.units_differ {
        event(Severity_::Warn, "SDC-DIFF", "the two files declare different set_units: values are compared as written".into(), b_path);
    }
    let counts = by_direction(&d);
    let msg = if equivalent {
        format!("sdc diff {a_path} against {b_path}: equivalent")
    } else {
        format!(
            "sdc diff {a_path} against {b_path}: {} change(s) ({} loosen, {} tighten, {} neutral, {} unknown), {} not compared",
            d.changes.len(),
            counts["loosens"],
            counts["tightens"],
            counts["neutral"],
            counts["unknown"],
            d.not_compared[0].len() + d.not_compared[1].len()
        )
    };
    event(if equivalent { Severity_::Info } else { Severity_::Warn }, "SDC-DIFF", msg, b_path);
    if want_json {
        println!(
            "{:#}",
            json!({
                "schema": "vyges-sdc/1.0",
                "command": "diff",
                "status": if equivalent { "ok" } else { "finding" },
                "sdc": a_path,
                "against": b_path,
                "equivalent": equivalent,
                "by_direction": counts,
                "changes": d.changes.iter().map(change_json).collect::<Vec<_>>(),
                "not_compared": { "sdc": d.not_compared[0], "against": d.not_compared[1] },
                "superseded": { "sdc": d.superseded[0], "against": d.superseded[1] },
                "units_differ": d.units_differ,
            })
        );
    } else {
        for c in &d.changes {
            println!("{}", render_change(c));
        }
    }
    i32::from(!equivalent)
}

fn cmd_map(args: &[String]) -> i32 {
    use vyges_loom::sdc_rtl_map;
    let want_json = args.iter().any(|a| a == "--json");
    let (Some(sdc_path), Some(nl_path), Some(map_path)) = (opt(args, "--sdc"), opt(args, "--netlist"), opt(args, "--map")) else {
        eprintln!("vyges-sdc map: --sdc <rtl.sdc>, --netlist <elab.v> (with --lib) and --map <names.json> are required");
        return 2;
    };
    let lib = match load_libs(&opts(args, "--lib")) {
        Ok(l) if !l.cells.is_empty() => l,
        Ok(_) => return fail(want_json, "map", sdc_path, "no cells in any --lib (the registers are found by their Liberty ff groups)"),
        Err(e) => return fail(want_json, "map", sdc_path, &e),
    };
    let map = match load_map(map_path) {
        Ok(m) => m,
        Err(e) => return fail(want_json, "map", sdc_path, &e),
    };
    let elab = match load_netlist(nl_path, args, &lib, sdc_path) {
        Ok(n) => n,
        Err(e) => return fail(want_json, "map", sdc_path, &e),
    };
    let ranges = match opt(args, "--ranges").map(load_ranges) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return fail(want_json, "map", sdc_path, &e),
        None => Default::default(),
    };
    let revised = match opt(args, "--revised").map(|p| load_netlist(p, args, &lib, sdc_path)) {
        Some(Ok(n)) => Some(n),
        Some(Err(e)) => return fail(want_json, "map", sdc_path, &e),
        None => None,
    };
    let ro = match read_opts(args, sdc_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vyges-sdc map: {e}");
            return 2;
        }
    };
    // read as written: the names are the RTL's, which the implementation does not have
    let sdc = match load_sdc(sdc_path, None, &ro) {
        Ok(s) => s,
        Err(e) => return fail(want_json, "map", sdc_path, &e),
    };
    emit_read(&sdc, sdc_path);
    let r = sdc_rtl_map::rtl_map(&sdc, &elab, &lib, &ranges, &map, revised.as_ref());
    let range = |d: &(i64, i64)| format!("[{}:{}]", d.0, d.1);
    let pat = |o: &sdc_rtl_map::ObjectRef| format!("{} {}", if o.accessor.is_empty() { "(bare)" } else { o.accessor.as_str() }, o.pattern.as_deref().unwrap_or("(all)"));
    for x in &r.resolved {
        if let sdc_rtl_map::BitIndex::Translated { to, declared } = &x.bits {
            event(Severity_::Info, "SDC-MAP-BIT", format!("{} is elaborated {to} (declared {})", pat(&x.object), range(declared)), sdc_path);
        }
    }
    for u in r.unknown() {
        let why = match &u.bits {
            sdc_rtl_map::BitIndex::OutOfRange { declared } => format!("index outside the declared range {}", range(declared)),
            _ => "names nothing in the elaborated design".to_string(),
        };
        event(Severity_::Warn, "SDC-MAP-UNKNOWN", format!("{} {why}: {}", pat(&u.object), u.object.line), sdc_path);
    }
    for (x, m) in r.uncarried() {
        event(
            Severity_::Warn,
            "SDC-MAP-UNCARRIED",
            format!("{} {} ({}) not carried: {}", pat(&x.object), m.name, m.kind.as_str(), m.reason.unwrap_or("")),
            sdc_path,
        );
    }
    for n in &r.not_resolved {
        event(Severity_::Warn, "SDC-MAP-UNCARRIED", format!("{} cannot be resolved without evaluating it: {}", pat(n), n.line), sdc_path);
    }
    for n in &r.not_written {
        event(Severity_::Warn, "SDC-NOT-WRITTEN", format!("objects not read: {n}"), sdc_path);
    }
    let (unknown, uncarried) = (r.unknown().count(), r.uncarried().count());
    let carried: usize = r.resolved.iter().map(|x| x.members.iter().filter(|m| m.carried.is_some()).count()).sum();
    let complete = r.complete();
    event(
        if complete { Severity_::Info } else { Severity_::Warn },
        "SDC-MAP",
        format!(
            "sdc map {sdc_path}: {carried} object(s) carried, {uncarried} not carried, {unknown} pattern(s) naming nothing, {} not resolved, {} command(s) not read",
            r.not_resolved.len(),
            r.not_written.len()
        ),
        sdc_path,
    );
    if want_json {
        let objects: Vec<Value> = r
            .resolved
            .iter()
            .map(|x| {
                json!({
                    "line": x.object.line,
                    "accessor": x.object.accessor,
                    "pattern": x.object.pattern,
                    "bits": match &x.bits {
                        sdc_rtl_map::BitIndex::Same => Value::Null,
                        sdc_rtl_map::BitIndex::Translated { to, declared } => json!({ "translated_to": to, "declared": [declared.0, declared.1] }),
                        sdc_rtl_map::BitIndex::OutOfRange { declared } => json!({ "out_of_range": true, "declared": [declared.0, declared.1] }),
                    },
                    "members": x.members.iter().map(|m| json!({
                        "name": m.name, "kind": m.kind.as_str(), "carried": m.carried, "reason": m.reason,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{:#}",
            json!({
                "schema": "vyges-sdc/1.0",
                "command": "map",
                "status": if complete { "ok" } else { "finding" },
                "sdc": sdc_path,
                "complete": complete,
                "counts": { "carried": carried, "not_carried": uncarried, "unknown": unknown, "not_resolved": r.not_resolved.len(), "not_written": r.not_written.len() },
                "objects": objects,
                "not_resolved": r.not_resolved.iter().map(|n| json!({ "line": n.line, "accessor": n.accessor, "pattern": n.pattern })).collect::<Vec<_>>(),
                "not_written": r.not_written,
            })
        );
    } else {
        for x in &r.resolved {
            match &x.bits {
                sdc_rtl_map::BitIndex::Translated { to, declared } => println!("bit          {} is elaborated {to} (declared {})", pat(&x.object), range(declared)),
                sdc_rtl_map::BitIndex::OutOfRange { declared } => {
                    println!("out of range {}  (declared {}; {})", pat(&x.object), range(declared), x.object.line)
                }
                sdc_rtl_map::BitIndex::Same if x.members.is_empty() => println!("unknown      {}  ({})", pat(&x.object), x.object.line),
                sdc_rtl_map::BitIndex::Same => {}
            }
            for m in &x.members {
                match &m.carried {
                    Some(c) => println!("carried      {:<12} {} -> {c}", m.kind.as_str(), m.name),
                    None => println!("not carried  {:<12} {}  ({})", m.kind.as_str(), m.name, m.reason.unwrap_or("")),
                }
            }
        }
        for n in &r.not_resolved {
            println!("not resolved {}  ({})", pat(n), n.line);
        }
        for n in &r.not_written {
            println!("not read     {n}");
        }
    }
    i32::from(!complete)
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
    // The suite's verbosity contract (vyges-cli `output-and-logging.md`), via the shared
    // `vyges_loom::verbosity`: `-q` warn, `-qq` error, `-qqq` off; `-v` debug, `-vv` trace;
    // repeatable, combining, and a flag OVERRIDES `VYGES_LOG` for this process.
    let (mut v, mut q) = (0u8, 0u8);
    for a in &args {
        match a.as_str() {
            "--verbose" => v += 1,
            "--quiet" => q += 1,
            s => {
                v += verbosity::short_flag(s, b'v');
                q += verbosity::short_flag(s, b'q');
            }
        }
    }
    verbosity::init(v, q);
    // ⛔ Before the first event: vyges-events reads `VYGES_LOG` once. `event` filters by the
    // level just resolved, so vyges-events passes all.
    std::env::set_var("VYGES_LOG", "trace");
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
        Some("diff") => cmd_diff(&args[1..]),
        Some("map") => cmd_map(&args[1..]),
        Some("waivers") => cmd_waivers(&args[1..]),
        Some(other) => {
            eprintln!("vyges-sdc: unknown command '{other}' (try --help)");
            2
        }
    }
}

/// `waivers export`: a waiver file written as another tool's Tcl waiver commands, by a template
/// (`vyges_loom::sdc_waivers::ExportTemplate`). A waiver the template cannot map is listed and not
/// written; exit 1 then, so nothing is dropped silently.
fn cmd_waivers(args: &[String]) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    if args.first().map(String::as_str) != Some("export") {
        eprintln!("vyges-sdc waivers: the subcommand is `export` (try --help)");
        return 2;
    }
    let (Some(wp), Some(tp)) = (opt(args, "--waivers"), opt(args, "--template")) else {
        eprintln!("vyges-sdc waivers export: --waivers <w.tcl> and --template <t.txt> are required");
        return 2;
    };
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let (wt, tt) = match (read(wp), read(tp)) {
        (Ok(w), Ok(t)) => (w, t),
        (Err(e), _) | (_, Err(e)) => return fail(want_json, "waivers export", wp, &e),
    };
    let t = match sdc_waivers::parse_template(&tt) {
        Ok(t) => t,
        Err(e) => return fail(want_json, "waivers export", wp, &format!("--template {tp}: {e}")),
    };
    let file = sdc_waivers::parse(&wt);
    let header = format!("Exported from {wp} with {tp} by vyges-sdc {VERSION} on {}.", sdc_waivers::utc_today());
    let e = sdc_waivers::export(&file, &t, &header);
    let mut skipped: Vec<String> = file.malformed.iter().map(|m| format!("{m}: not a valid waiver")).collect();
    skipped.extend(e.skipped.iter().cloned());
    for k in &skipped {
        event(Severity_::Warn, "SDC-WAIVER-SKIPPED", format!("not exported: {k}"), wp);
    }
    let written = file.waivers.len() - e.skipped.len();
    let out = opt(args, "-o").or_else(|| opt(args, "--out"));
    if let Some(o) = out {
        // never over another file
        let w = std::fs::OpenOptions::new().write(true).create_new(true).open(o).and_then(|mut f| std::io::Write::write_all(&mut f, e.text.as_bytes()));
        if let Err(err) = w {
            return fail(want_json, "waivers export", wp, &format!("-o {o}: {err} (it is never overwritten; name a new file)"));
        }
    }
    let sev = if skipped.is_empty() { Severity_::Info } else { Severity_::Warn };
    event(sev, "SDC-WAIVERS-EXPORT", format!("waivers export of {wp}: {written} written, {} not exported", skipped.len()), wp);
    if want_json {
        let mut v = json!({ "schema": "vyges-sdc/1.0", "command": "waivers export", "status": "ok",
                            "waivers": wp, "template": tp, "written": written, "skipped": skipped });
        if out.is_none() {
            v["text"] = json!(e.text);
        }
        println!("{v:#}");
    } else if out.is_none() {
        print!("{}", e.text);
    }
    i32::from(!skipped.is_empty())
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
        assert_eq!(d["tools"][1]["name"], "sdc.diff");
        assert_eq!(d["tools"][2]["name"], "sdc.map");
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
