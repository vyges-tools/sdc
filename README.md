# vyges-sdc

A **Loom engine** for **SDC timing constraints**: it checks them, writes them, and compares two
sets of them.

A timer computes timing on whatever constraints it is given. A constraint that is wrong does not
produce a wrong-looking report; it produces a **clean** one. An output with no
`set_output_delay` is a path nobody checks. A clock on a port that drives nothing leaves every
register untimed while the SDC still looks populated. A `set_false_path` with a typo names
nothing and quietly does nothing. `vyges-sdc check` looks for exactly these.

Constraints also change: an edit, a new delivery, a netlist flow's copy of the RTL one. A text diff
of two constraint files shows every reordering and reformatting and hides what matters.
`vyges-sdc diff` says which constraints changed and whether each change loosens or tightens what
is checked.

Some findings are intended. A **waiver** records that decision as an open Tcl file of
`waive_finding` commands, one per finding, keyed by an id that survives edits elsewhere in the
SDC. `check --waivers` applies them and reports any that went stale or expired, and `waivers
export` writes them as another tool's waiver commands, by a template you supply.

## Usage

<!-- usage:begin -->
```sh
vyges physical sdc check --sdc design.sdc [--netlist design.v --lib cells.lib [--lib ...]]
                         [--top NAME] [--keep MODULE ...] [--metadata vyges-metadata.json]
                         [--define NAME=VALUE ...] [--waivers waivers.tcl]
                         [--write-waivers new-waivers.tcl] [--fail-on-warning] [--json]
                         [-q|--quiet ...] [-v|--verbose ...]
vyges physical sdc write --sdc design.sdc [-o|--out normalised.sdc]
                         [--netlist design.v --lib cells.lib [--lib ...]]
                         [--top NAME] [--keep MODULE ...] [--define NAME=VALUE ...]
                         [--design NAME] [--partial] [--json] [-q|--quiet ...] [-v|--verbose ...]
vyges physical sdc diff --sdc old.sdc --against new.sdc [--map names.json]
                        [--netlist design.v --lib cells.lib [--lib ...]]
                        [--top NAME] [--keep MODULE ...] [--define NAME=VALUE ...]
                        [--json] [-q|--quiet ...] [-v|--verbose ...]
vyges physical sdc waivers export --waivers waivers.tcl --template target.txt [-o out.tcl] [--json]
vyges physical sdc --describe | --version | --help
vyges physical sdc --bug-report | --feature-request | --sponsor | --star
```
<!-- usage:end -->

`vyges-sdc --help` describes each option; a test fails when this block and `--help` name
different options.

### `check`

`check` tests the constraints against the netlist, with the Liberty to say which cells are
registers:

- **Completeness:** inputs and outputs with no I/O delay, registers that no clock reaches, and
  endpoint coverage (the share of timing endpoints that are constrained at all), reported on
  every run, clean ones included.
- **Consistency:** duplicate clock names, clocks on ports the design does not have, and
  exceptions (`set_false_path`, `set_multicycle_path`) that name no object or no structural path.
- **Overlapping exceptions:** an exception another one outranks on every path it names
  (shadowed), two that disagree at the same priority (conflict), and one that adds nothing
  (redundant), with the winner decided as the reference timer decides it. Needs no netlist.
- **Clock groups:** a generated clock left out of the groups its master is in, two clocks of
  one family declared asynchronous, and words of `set_clock_groups` the reference timer
  ignores (an option it lacks, a `-group` name that matches no clock). Needs no netlist.
- **Declared clock domains:** given `--metadata`, the IP's `clock_domains` from
  `vyges-metadata.json` are checked against the SDC's clocks. A three-clock IP with one
  `create_clock` passes timing and times only part of the design; this catches it.

Every check also reports each command that was read but **not applied**: one the model does not
carry, one it refused, or one whose object query it could not evaluate. Such a command is an
error when it affects timing (a constraint the timer never sees is a hole in the constraints) and
information when it does not (a synthesis directive).

It reports, too, each command that is **kept** (read and written back) but applied by no checker
or timer here, with its count: `set_clock_gating_check`, the wire-load and operating-condition
commands, `set_sense`, `set_clock_sense`, for example (the coverage table below marks them Kept). That is a warning
when applying it could move a slack, so the report is not called clean, and information when it
could not (the area and power limits `set_max_area`, `set_max_dynamic_power`,
`set_max_leakage_power`). It is not an error, since the command was read as the reference timer
reads it. The JSON result counts them in `kept_not_applied`.

It reports **overlapping exceptions**, with the winner on the shared paths decided as the
reference timer decides it: the type first (a false path, then a max or min delay, a multicycle, a
path margin, a path group), then the points (`-from` pins, `-to` pins, `-through`, `-from` clocks,
`-to` clocks), with a multicycle's `-setup` or `-hold` counting in the check it names.

| code | severity | meaning |
|---|---|---|
| `exception-shadowed` | warning | Another exception covers every path this one names and outranks it, so it never applies |
| `exception-conflict` | warning | Another covers the same paths at the same priority with a different value: the one read later replaces the earlier, or the tighter value wins, or the reference has no defined choice |
| `exception-redundant` | information | Another covers every path it names and says the same, so it adds nothing |
| `exception-overlap-not-checked` | information | Exceptions not checked for overlap: a file with `-reset_path`, or objects named in a way the check cannot classify (a bare name list), with the first one named |

Two rules that are easy to get wrong:

- Two multicycles, or two max delays, with the same priority and the same kinds of points: the later
  one in the file replaces the earlier one. It does not matter which is tighter.
- A `-setup` multicycle also moves the hold check. A `-setup` false path or a `set_max_delay` over
  it leaves that effect, so it is not reported.

An overlap is reported only when it can be shown from the file: each point of the covering
exception names, with the same query, every object the covered one names (a pattern may match
them). A clock that launches a pin, or a cell that owns a pin, is not resolved, so the check can
miss an overlap but does not report a false one.

It reports **clock groups that do not say what they appear to**, with the groups read as the
reference timer reads them: a group holds exactly the clocks its `-group` names (each word a
pattern over the clocks defined before the command), and a command with one group separates its
clocks from every other clock.

| code | severity | meaning |
|---|---|---|
| `clock-generated-ungrouped` | warning | A generated clock is in no group of a command that groups its master: it is timed against the other groups, or, under a one-group command, cut from its own master |
| `clock-related-declared-async` | warning | Two clocks of one family (one generated from the other, or both from one clock) are in different `-asynchronous` groups: their paths go untimed, which is right only if every crossing between them is synchronized |
| `clock-relations-not-checked` | information | Generated clocks whose master only the design can find (no `-master_clock`, no clock on the `-source`), with the first one named |
| `sdc-word-ignored` | warning | A word the reference timer warns about and ignores while applying the command: an option `set_clock_groups` does not have (`-include_generated_clocks`), a stray word, or a `-group` name that matches no clock defined before it |

It names, at information level and with a count, each command or option the file uses that the
reference timer reads but no SDC version declares (`get_clocks -filter`, `-reset_path`, `set_pvt`,
the `unset_*` commands; the last table below lists the options). The file is read as the
reference timer reads it; another SDC reader may refuse it.

**Waivers** are a Tcl file of `waive_finding` commands, one per accepted finding, meant to sit
with the other Tcl files a flow sources:

```tcl
waive_finding -id exception-shadowed:3f2a9c1d07be -code exception-shadowed \
    -objects {{set_multicycle_path 3 -from [get_pins r1/CK]} {set_false_path -from [get_pins r1/CK]}} \
    -reason {the false path is intended; the multicycle is left for the next netlist} \
    -owner {jdoe} -expires 2027-06-30
```

- `--write-waivers FILE` writes one such command for every error and warning still active, with
  the finding above it as a comment and an empty `-reason` to fill in. It never overwrites a file.
- `--waivers FILE` applies them. A waived finding is listed under `waived`, with its reason, and
  counts as neither an error nor a warning.
- The `-id` is the finding's identity: its code and what it is about. It does not depend on line
  numbers or on the order of the file, so it survives edits elsewhere, and it changes when what
  the finding is about changes.
- A waiver that waives nothing is a warning of its own: `waiver-stale` (it matches no finding),
  `waiver-no-reason`, `waiver-expired` (`-expires` is a `YYYY-MM-DD` date, in UTC, and the waiver
  applies through that day), `waiver-duplicate` and `waiver-malformed`.
- Another tool can `source` the file after defining `waive_finding`, or read it as data: it holds
  no other Tcl. Only `waive_finding` is read.

**Without `--netlist`**, `check` runs what the file alone supports: the not-applied report, clock
periods and duplicate names, overlapping exceptions, plus one note that the design checks did not run.

**Hierarchical netlists** are flattened before checking, as the timer reads them, so a constraint
on `u_core/u_alu/...` finds its object. `--top NAME` names the top module when the file has more
than one candidate (otherwise the read is refused, listing them), and `--keep MODULE` keeps a
module as a block.

**`--define NAME=VALUE`** (repeatable) sets a Tcl variable before the file is read. Synthesis flows
often define names such as a driving cell or a period factor in a setup script that the constraint
file only uses.

The exit code is 1 when there is any error. Warnings are reported and leave it at 0.

```jsonc
{ "command": "check", "clean": true, "errors": 0, "warnings": 2,
  "endpoint_coverage_pct": 50.0,
  "findings": [
    { "severity": "warning", "code": "endpoint-coverage", "message": "endpoint coverage 50.0% — 1 of 2 constrained (1/1 registers clocked, 0/1 outputs with a delay)" },
    { "severity": "warning", "code": "unconstrained-output", "message": "output `dout` has no set_output_delay" } ] }
```

### `write`

`write` reads the SDC into the shared constraint model and writes it back out in canonical form:
- in the order a timer writes its own constraint database;
- with each value in the shortest form that reads back to the same number;
- in the units the file declared.

A netlist (and Liberty) lets object queries that need the design (`-filter`, `-of_objects`,
`-regexp`) be evaluated rather than refused.

**Nothing is dropped silently.** If any command would be left out (one the model does not carry,
or one it refused or could not evaluate), `write` writes **no file**. It exits 1 and lists every
such command. With `--partial` it writes the rest, and each command it left out appears in the
file as a `# not written:` line. The JSON result says `complete: false` either way.

```jsonc
{ "command": "write", "status": "refused", "complete": false,
  "not_written": [ "not modelled: set_assigned_delay" ] }
```

Malformed constraints are refused when the file is read rather than guessed at. For example, a
`-period` given with a unit (`20ns`, `"1 MHz"`) is an error, because SDC periods are plain
numbers in the file's time unit. A design-rule limit (`set_max_capacitance`, `set_max_fanout`,
`set_max_transition`) without its value or its objects, with an option it does not have, or with a
value that is not a positive number is refused with a message, and `check` reports it as an
error: `set_max_capacitance 0.2` (no objects) and `set_max_fanout -clock_path 8 [get_clocks c]`
(`set_max_fanout` has no option) are both errors. `set_max_capacitance -clock_path 0.1 [get_clocks
c]` is SDC 1.8 in newer releases of the vendor's SDC parser (†): it is read, kept as written and
reported as kept but not applied.

### `diff`

`diff` compares two SDC files as constraints, not as text: an edit against the next, one
delivery against the next, or an RTL constraint file against the netlist one. It reports each
constraint that was added, removed or changed, and what the change does to what is checked:

| Direction | Means | Examples |
| --- | --- | --- |
| loosens | fewer or easier checks | a longer period; a new false path; a larger multicycle; a smaller `-max` I/O delay; a removed clock or I/O delay |
| tightens | more or harder checks | a shorter period; a larger uncertainty; a removed false path; a larger load |
| neutral | nothing checked changes | a changed `-comment` |
| unknown | no rule decides it | one I/O delay for both `-min` and `-max` (tighter setup, looser hold); a waveform; a latency; an unmodelled command |

A gate that must not let constraints loosen unnoticed treats `unknown` as it treats `loosens`.

**What is a difference, and what is not.** Each constraint is identified by what it is ON: its
command, its flags, the options that name what it applies to (`-clock`, `-from`, `-to`,
`-through`, `-name`, …) and its objects. A clock is identified by its name alone, so a new
period or source is a change to that clock, not a new one. What it SAYS (the numbers, the other
options, `-comment`) is compared field by field. These are not differences:

- the order of commands, or of names inside a list;
- one command on several ports, against one command per port;
- a value a later command in the same file overrides, as SDC's own merge does; the overridden
  command is listed as `superseded`.

**Objects** are compared as written. With `--netlist`, both files are read against the design, so
queries are compared by the objects they resolve to. `--map` renames the first file's objects
before comparing: a JSON object of old name to new name, matched against a whole name or the
instance part of a pin, longest first. That is how an RTL constraint file is compared with a
netlist one.

```sh
vyges physical sdc diff --sdc rtl.sdc --against netlist.sdc --map names.json
```
```text
changed  loosens   create_clock -name io_aclk  -period 5000 -> 6000
changed  tightens  set_clock_uncertainty -setup get_clocks {all_clocks}  value 250 -> 300
removed  tightens  set_false_path -from get_ports -quiet {io_aresetn}  set_false_path -from [get_ports -quiet io_aresetn]
```

**Commands the model does not carry are compared by their text.** An `if` block that needs the
design to evaluate, or a command the reader reads past, is compared as the reader read it: its
text, with whitespace collapsed, its variables substituted, and the values of the variables it
names. The same text on both sides is the same constraint; a text on one side only is a change of
direction `unknown`.

**What cannot be compared is said.** A rejected command, or any unmodelled command when `--map` is
given (a raw text cannot be renamed faithfully), is listed under `not_compared`, and the two files
are then never reported as equivalent, even when no change was found. Values are compared in the
units each file declared; when the two declare different `set_units`, `units_differ` says so.

```jsonc
{ "command": "diff", "status": "finding", "equivalent": false,
  "by_direction": { "loosens": 0, "tightens": 1, "neutral": 0, "unknown": 0 },
  "changes": [ { "kind": "removed", "direction": "tightens", "command": "set_false_path",
                 "key": "set_false_path -from get_ports -quiet {io_aresetn}", "fields": [],
                 "a": "set_false_path -from [get_ports -quiet io_aresetn]", "b": null } ],
  "not_compared": { "sdc": [], "against": [] },
  "superseded": { "sdc": [], "against": [] }, "units_differ": false }
```

Exit 0 means equivalent; exit 1 means the files differ or something was not compared.

### `waivers export`

`waivers export` writes a waiver file as the Tcl waiver commands another timing, constraint or
lint tool sources. A **template** says how: the command line written per waiver, how each object
is written, and which of that tool's rule ids each of our finding codes stands for.

```text
# one line per waiver; a part in [[...]] is written only when its values are present
command  my_waive -status {Waived} -rule {@rule@} -match {@objects@} -comment {@reason@}[[ -owner {@owner@}]]
object   name=~"@object@"
join     \s||\s
rule     exception-shadowed   OVERLAP_SHADOW
rule     exception-conflict   OVERLAP_CONFLICT OVERLAP_VALUE
```

- Placeholders in `command`: `@rule@`, `@objects@`, `@reason@`, `@owner@`, `@expires@`, `@id@`,
  `@code@`. In `object`: `@object@`, or `@object_re@` with regular-expression metacharacters
  escaped (for a target that matches by regexp). `join` goes between objects (`\s`, `\t`, `\n` for a blank,
  a tab, a newline).
- `rule * …` maps every code not mapped otherwise, and `@code@` in it is our own code.
- A waiver whose code has **no mapping is not written**. It is listed, and the exit code is 1:
  a guessed rule id would be a waiver the other tool silently ignores. A waiver with no reason,
  or a command that is not a valid waiver, is listed the same way.
- An unknown directive or placeholder in the template is an error, so a typo cannot reach the
  other tool's file. A value whose braces do not balance is escaped, so every command still
  parses as Tcl.
- `-o` never overwrites a file. Without it the result goes to stdout (with `--json`, the
  envelope's `text`).

This template writes our own format back, which is how the export is tested:

```text
command waive_finding -id @id@ -code @code@ -objects {@objects@} -reason {@reason@}[[ -owner {@owner@}]][[ -expires @expires@]]
object  {@object@}
rule    * @code@
```

## Constraints

These are the constraints `write` writes back, read with the semantics of the reference timer.
The list is checked against what the engine actually writes, so it cannot claim more than the
code does.

<!-- constraints-written:begin -->
| Group | Commands |
| --- | --- |
| Clocks | `create_clock`, `create_generated_clock`, `set_clock_uncertainty`, `set_clock_latency`, `set_clock_transition`, `set_propagated_clock`, `set_clock_groups` |
| Inputs and outputs | `set_input_delay`, `set_output_delay`, `set_input_transition`, `set_load`, `set_driving_cell` |
| Timing exceptions | `set_false_path`, `set_multicycle_path`, `set_max_delay`, `set_min_delay` |
| Constant logic | `set_case_analysis`, `set_logic_zero`, `set_logic_one`, `set_logic_dc` |
| Derating and units | `set_timing_derate`, `set_units` |
| Design rules | `set_max_transition`, `set_max_fanout`, `set_max_capacitance` |
| Arcs and senses | `set_disable_timing`, `unset_disable_timing`, `set_sense`, `set_clock_sense`, `set_disable_clock_gating_check`, `unset_disable_clock_gating_check` |
<!-- constraints-written:end -->

**Objects.** `get_ports`, `get_pins`, `get_cells`, `get_nets` and `get_clocks` with their
patterns, `all_inputs`, `all_outputs`, `all_clocks`, `all_registers`, and Tcl lists of these
(`[list …]`, `[concat …]`). With a netlist, queries that select by property or connectivity
(`-filter`, `-of_objects`, `-regexp`) are evaluated against the design. Every input or output but
some is read in both common spellings:
`[lsearch -inline -all -not -exact [all_inputs] [get_ports clk]]` and
`[remove_from_collection [all_inputs] clk]`. Options no SDC version declares, such as
`get_pins -leaf` and `get_nets -segments`, are refused (the command is not applied), as the
reference timer refuses them. With the libraries and a netlist, `[load_of lib/cell/pin]` is that
library pin's capacitance; without them its command is not applied.

**SDC options the reference timer lacks** are read, never refused: `-level_sensitive` on
`set_input_delay` / `set_output_delay`, `-min` / `-max` on `set_timing_derate`, `-clock` /
`-clock_fall` on `set_driving_cell`, the `all_inputs`, `all_outputs` and `all_registers` options,
`-unit` on the power limits, `-object_list` on `set_operating_conditions` and `-group_name` on
`set_wire_load_selection_group`. Each is written back, and `check` reports it as kept but not
applied: a command whose meaning it changes (a clock-scoped drive, an input list selected by
clock) is kept as written rather than applied as if the option were not there. `get_ports
-hierarchical` is `get_ports` (a port is at the top level), and `set_load -substract_pin_load` is
`-subtract_pin_load`, and `set_false_path -rise_throough` / `-fall_throough`, the public
parser's own spelling, are `-rise_through` / `-fall_through`. The table at the end of the command
coverage lists them.

**SDC 1.8 in newer vendor releases.** Vendor SDC 1.8 declares more than the public release of
the vendor's SDC parser, and how much more varies by release (†): `-filter` and `-quiet` on the
object and library queries, `get_pins -of_objects`, `set_load -rise` / `-fall` / `-internal` /
`-allow_negative_cap`, `set_max_transition -data_path` / `-delay_penalty` (a flag, taking no
value) / `-force`, `set_max_capacitance -clock_path` / `-data_path` / `-rise` / `-fall` /
`-force` / `-sms_scenarios`, the collection commands
(`add_to_collection`, `append_to_collection`, `remove_from_collection`, `filter_collection`) and
`all_fanin` / `all_fanout`. They are SDC 1.8 here too: read, never refused, and not named as
reference-timer extensions. An option the reference timer has is applied as it applies it; the
others are kept with their command and reported as kept but not applied. Every option any SDC
version declares, public or vendor, is read by a test that fails on a refusal.

**Tcl.** The subset constraint files use: `set` and variables (`$name`, `${name}`,
`$::env(NAME)`), `if`/`elseif`/`else`, `foreach` over a list, `switch`, `expr`, `proc` (each call
runs its body with its parameters bound and its own local variables; `return` ends it, and a proc
called inside `[…]` stands for the value it returns), `global`, `upvar` (the caller's variable,
written back when the proc returns), `dict` (`create`, `set`, `get`, `exists`, `keys`, `values`,
`size`, `merge`, `unset`, `lappend`, `append`, `incr` and `for`, in insertion order), `source`
(read in place, from the working directory and then from the reading file's directory; a `return`
at a file's top level ends that file), `puts`, and abbreviated command and option names, expanded
as the reference expands them. Nothing is substituted inside braces, as in Tcl. Reading a variable
that is not defined is Tcl's error and stops the read there (exit 2), as it stops the reference
timer: `can't read "X": no such variable`. A flow's variables are given with `--define`. The singular
query spellings (`get_port`, `get_cell`, …) are the plural queries.

**Files written for another tool.** Constraint files that DFT insertion and signoff tools write
test which tool is reading them. `$synopsys_program_name` and `[get_app_var
synopsys_program_name]` read as a signoff timer's name unless the flow defines the variable
(`info exists` sees only what is defined). The signoff timer's take-back commands are read as the
reference's: `remove_input_delay`, `remove_output_delay`, `remove_propagated_clock`,
`remove_clock_uncertainty`, `remove_clock_latency`, `remove_clock_transition`,
`remove_case_analysis`, `remove_clock_groups`, `remove_disable_timing`, `remove_data_check`,
`remove_clock`, `remove_generated_clock`, `reset_path` and `reset_timing_derate`. Without
`-clock`, `remove_input_delay` and `remove_output_delay` take back every delay on the objects,
whatever its clock, as the signoff timer does.

**Synthesis-tool collections.** `append_to_collection` and `add_to_collection` build object lists,
and `get_object_name` gives their names. A `foreach_in_collection` loop whose body hands each
object to constraints as their object is those constraints on the whole collection.
`get_designs`, `filter_collection`, `get_attribute` (also abbreviated, `get_attr`),
`sizeof_collection`, `index_collection`, `sort_collection`, `all_fanin`, `all_fanout`, and a
`foreach_in_collection` loop that needs its objects one by one are recognised: the command using
one is reported as not applied, and the constraints after it are still read.

**Synthesis directives** are recognised and read past: `set_size_only`, `set_critical_range`,
`set_dont_touch_network`, `set_optimize_design`, `set_scan_configuration`, `set_min_porosity`,
`set_clock_gating_enable`, `set_clock_gating_style`, `set_clock_gating_verification` and
`set_timing_enable_verification`. They steer optimisation and change no timing result, so the
constraints after them are applied. `write` lists each one as not written, since the file it
writes is for timing; `--partial` writes the rest.

**Everything else** the reference reads is read too. Constraints no checker or timer here applies
yet (`set_min_pulse_width`, `set_max_time_borrow`, the wire-load and
operating-condition commands, for example) are kept and written back as they were read, and
`check` reports each one. A command
the reference runs but that is no constraint (a tool setting such as `set_assigned_delay`) is
reported by name: `check` reports it, and `write` lists it as not written. A command that is not SDC or Tcl at all stops the read with an error that names it and
the line it is on (exit 2). The reference timer stops there too, and a constraint file read only
partly would otherwise be timed as if it were whole.

## SDC command coverage

Every command of the SDC command set, with the SDC version that introduced it and what this
engine does with it. The version is the first SDC version whose declaration file, in the
reference SDC parser Synopsys published with the standard (SDC 1.1 to 2.0), declares the command:
the singular query spellings (`get_cell`, `get_port`, …) are SDC 1.5, though their plurals are
1.1. ‡ 2.1, per the reference timer's documentation; no public SDC 2.1 document. † Declared in
that version by the vendor's releases of the parser, and by none of the public release's files:
releases differ, and newer ones declare more in SDC 1.8 (see "SDC 1.8 in newer vendor releases"
above); a command or option marked † is SDC here. `set_current_design` (1.1 †) is read as
`current_design`; with a netlist, `check` warns when either names a design other than the
netlist's top (`current-design-not-top`), and the constraints are applied to the top.

The public release these versions are dated from is the copy distributed in the OpenTimer
project's `ot/sdc/` directory (commit `cd0565f`), whose declaration files are:

| File | SHA-256 |
| --- | --- |
| `sdc1.1.tcl` | `541d1d685eb1890bdc52fe00b2b6b79c2c39c5ea2405dfa43f6259b11c05e6ae` |
| `sdc1.2.tcl` | `c1aa2d55062b3207b3ad38fe20adb3beb08584c5074e156231cb62095f5449df` |
| `sdc1.3.tcl` | `a67592ac8c7a505d537a16b3a320d7f27a8848e0f60e4d693f12471a898702b9` |
| `sdc1.4.tcl` | `11a6635a64648af715fd895faa058ce702959f26057b282220f3d1b9a8aec0d0` |
| `sdc1.5.tcl` | `31905d514e2055b8ebea1c48abd8100966fed154879ac1c0e3e12cb967242e49` |
| `sdc1.6.tcl` | `8ff3ed2add57486393da8145fc7ba06d0bc80b0b3d901ab114e64309feb9dc09` |
| `sdc1.7.tcl` | `7d5443e13493a84dda9a7d46014ebac676b64ea315f58e6f852dda0c5e46154c` |
| `sdc1.8.tcl` | `b5944629e9610a1123f3a7329d911a8bb53223370dd12405780502ae72422056` |
| `sdc1.9.tcl` | `5f175e7416d4ee0425e747e008bc4b723be9c383c262cd686f943a600c77abf5` |
| `sdc2.0.tcl` | `6f0aaf1a4d4125b131c4965928d21994323ddf0bce5ea85470d3754005520c91` |

`set_clock_jitter` and `set_disable_clock_gating_check` are declared by no version the
parser covers, and are listed outside the set. The tables are generated from the command registry in loom
(`vyges_loom::sdc::registry`), whose own test reads one probe line per command and fails if a
status is not what the reader does. `cargo run --example coverage_table` prints them, and a test
in this repository fails when the README's copy differs. "Applied by" names the consumers'
code: change it with the consumer.

- **Applied**: read into the constraint model, written back, and applied by the tool named under
  "Applied by": `check` (this engine's check), `sta` (the `vyges-tools/sta` timer), `sta-si` (the
  `vyges-tools/sta-si` timer), `rsz` (the `vyges-tools/rsz` design repair, for the design-rule
  limits), or `reader` (evaluated while the file is read: object queries, units, the current
  design and instance, and take-back commands; what it changes is applied where the command it
  changes is).
- **Kept**: read as the reference timer reads it and written back, but applied by no checker or
  timer here. `check` reports each one it reads, with its count.
- **Tcl**: run as Tcl runs it.
- **Read past**: a synthesis-only command the reference timer does not run (`set_dont_touch`,
  `set_min_porosity`; outside SDC, `set_size_only` and the like). It is skipped, reported as not
  applied at information level, and `write` lists it as not written (`--partial` writes the
  rest). One rule covers the area and power directives: one the reference timer reads is Kept
  (written back, reported at information level), one it does not is Read past.
- **Not applied**: recognised but not evaluated (a fan-in query, a collection filter without the
  design): the command that uses it is reported as not applied, as an error in `check` when it
  can move a slack, and the read goes on.
- **Unhandled**: not read; the file stops there (exit 2), as it does in the reference timer.

<!-- sdc-coverage:begin -->
SDC 1.1 to 2.1: 57 applied, 25 kept, 4 Tcl, 2 read past, 4 not applied, 0 unhandled, of 92 commands.

| Command | SDC | Status | Applied by |
| --- | --- | --- | --- |
| `add_to_collection` | 1.8 † | Applied | `reader` |
| `all_clocks` | 1.1 | Applied | `reader` |
| `all_fanin` | 1.8 † | Not applied | — |
| `all_fanout` | 1.8 † | Not applied | — |
| `all_inputs` | 1.1 | Applied | `reader` |
| `all_outputs` | 1.1 | Applied | `reader` |
| `all_registers` | 1.7 | Applied | `reader` |
| `append_to_collection` | 1.8 † | Applied | `reader` |
| `create_clock` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `create_generated_clock` | 1.3 | Applied | `check`, `sta`, `sta-si` |
| `create_voltage_area` | 1.6 | Kept | — |
| `current_design` | 1.1 | Applied | `reader` |
| `current_instance` | 1.1 | Applied (`-instance` or the positional; an unqualified name under it, and `-hierarchical` under it, are refused) | `reader` |
| `expr` | Tcl | Tcl | — |
| `filter_collection` | 1.8 † | Not applied | — |
| `get_cell` | 1.5 | Applied (spelling of `get_cells`) | `reader` |
| `get_cells` | 1.1 | Applied | `reader` |
| `get_clock` | 1.5 | Applied (spelling of `get_clocks`) | `reader` |
| `get_clocks` | 1.1 | Applied | `reader` |
| `get_designs` | 1.5 † | Not applied | — |
| `get_lib_cell` | 1.5 | Applied (spelling of `get_lib_cells`) | `reader` |
| `get_lib_cells` | 1.1 | Applied | `reader` |
| `get_lib_pin` | 1.5 | Applied (spelling of `get_lib_pins`) | `reader` |
| `get_lib_pins` | 1.1 | Applied | `reader` |
| `get_libs` | 1.1 | Applied | `reader` |
| `get_net` | 1.5 | Applied (spelling of `get_nets`) | `reader` |
| `get_nets` | 1.1 | Applied | `reader` |
| `get_pin` | 1.5 | Applied (spelling of `get_pins`) | `reader` |
| `get_pins` | 1.1 | Applied | `reader` |
| `get_port` | 1.5 | Applied (spelling of `get_ports`) | `reader` |
| `get_ports` | 1.1 | Applied | `reader` |
| `group_path` | 1.7 | Applied (`sta`: `-name` groups, their path ends reported; no slack moves with it) | `sta` |
| `if` | Tcl | Tcl (a condition the file alone decides (`info exists`, a variable); one on a query's result needs the design and is listed as not applied) | — |
| `list` | Tcl | Tcl | — |
| `remove_from_collection` | 1.8 † | Applied (only as every input or output but some, the objects of an I/O delay) | `reader` |
| `remove_propagated_clock` | 1.1 † | Applied | `reader` |
| `set` | Tcl | Tcl | — |
| `set_case_analysis` | 1.1 | Applied (`sta`: through its logic simulation, with the design) | `sta`, `sta-si` |
| `set_clock_gating_check` | 1.2 | Kept | — |
| `set_clock_group` | 1.7 | Applied (spelling of `set_clock_groups`) | `sta`, `sta-si` |
| `set_clock_groups` | 1.7 | Applied | `sta`, `sta-si` |
| `set_clock_latency` | 1.1 | Applied (`sta-si`: only as included in an I/O delay (`-source_latency_included`, `-network_latency_included`)) | `sta`, `sta-si` |
| `set_clock_sense` | 1.7 | Kept | — |
| `set_clock_transition` | 1.1 | Applied | `sta`, `sta-si` |
| `set_clock_uncertainty` | 1.1 | Applied | `sta`, `sta-si` |
| `set_current_design` | 1.1 † | Applied (read as `current_design`; `check` warns when it names a design other than the netlist's top) | `reader` |
| `set_data_check` | 1.4 | Applied (`-rise` / `-fall`: kept, not applied) | `sta-si` |
| `set_disable_timing` | 1.1 | Applied | `sta-si` |
| `set_dont_touch` | 1.1 † | Read past | — |
| `set_drive` | 1.1 | Kept | — |
| `set_driving_cell` | 1.1 | Applied | `sta`, `sta-si` |
| `set_false_path` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_fanout_load` | 1.1 | Kept | — |
| `set_hierarchy_separator` | 1.2 | Applied (`/` only: another stops the read) | `reader` |
| `set_ideal_latency` | 1.7 | Kept | — |
| `set_ideal_network` | 1.7 | Kept (SDC's `-no_propagate` and the reference's `-no_propagation` both read) | — |
| `set_ideal_transition` | 1.7 | Kept | — |
| `set_input_delay` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_input_transition` | 1.1 | Applied | `sta`, `sta-si` |
| `set_level_shifter_strategy` | 1.6 | Kept | — |
| `set_level_shifter_threshold` | 1.6 | Kept | — |
| `set_load` | 1.1 | Applied | `sta`, `sta-si` |
| `set_logic_dc` | 1.1 | Applied (`sta`: through its logic simulation, with the design) | `sta`, `sta-si` |
| `set_logic_one` | 1.1 | Applied (`sta`: through its logic simulation, with the design) | `sta`, `sta-si` |
| `set_logic_zero` | 1.1 | Applied (`sta`: through its logic simulation, with the design) | `sta`, `sta-si` |
| `set_max_area` | 1.1 | Kept | — |
| `set_max_capacitance` | 1.1 | Applied | `rsz` |
| `set_max_delay` | 1.1 | Applied | `check`, `sta` |
| `set_max_dynamic_power` | 1.4 | Kept | — |
| `set_max_fanout` | 1.1 | Applied | `rsz` |
| `set_max_leakage_power` | 1.4 | Kept | — |
| `set_max_time_borrow` | 1.1 | Kept | — |
| `set_max_transition` | 1.1 | Applied | `rsz` |
| `set_min_capacitance` | 1.1 | Kept | — |
| `set_min_delay` | 1.1 | Applied | `check`, `sta` |
| `set_min_porosity` | 1.4 | Read past | — |
| `set_min_pulse_width` | 2.0 | Kept | — |
| `set_multicycle_path` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_operating_conditions` | 1.1 | Kept | — |
| `set_output_delay` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_port_fanout_number` | 1.1 | Kept | — |
| `set_propagated_clock` | 1.1 | Applied | `sta`, `sta-si` |
| `set_resistance` | 1.1 | Kept | — |
| `set_sense` | 2.1 ‡ | Kept (without `-type`: read as `-type clock`, the SDC 2.1 default; `-type data` is not modelled) | — |
| `set_timing_derate` | 1.5 | Applied | `sta`, `sta-si` |
| `set_unit` | 1.7 | Applied (spelling of `set_units`) | `reader` |
| `set_units` | 1.7 | Applied | `reader` |
| `set_voltage` | 1.8 | Kept | — |
| `set_wire_load_min_block_size` | 1.1 | Kept | — |
| `set_wire_load_mode` | 1.1 | Kept | — |
| `set_wire_load_model` | 1.1 | Kept | — |
| `set_wire_load_selection_group` | 1.1 | Kept | — |

Commands outside the SDC versions the reference parser declares, found in constraint files written for other tools (a synthesis tool's collection commands among them):

| Command | Status | Applied by |
| --- | --- | --- |
| `delete_clock` | Applied | `reader` |
| `delete_generated_clock` | Applied | `reader` |
| `derive_clock_uncertainty` | Unhandled (known, not run: it stops the read) | — |
| `derive_pll_clocks` | Unhandled (known, not run: it stops the read) | — |
| `foreach_in_collection` | Applied | `reader` |
| `get_attribute` | Not applied | — |
| `get_object_name` | Applied | `reader` |
| `index_collection` | Not applied | — |
| `load_of` | Not applied (with the libraries and a netlist: the library pin's capacitance; without them, not applied) | — |
| `set_clock_jitter` | Unhandled | — |
| `set_disable_clock_gating_check` | Kept | — |
| `set_path_margin` | Kept | — |
| `set_pvt` | Kept | — |
| `sizeof_collection` | Not applied | — |
| `sort_collection` | Not applied | — |
| `unset_case_analysis` | Applied | `reader` |
| `unset_clock_groups` | Applied | `reader` |
| `unset_clock_latency` | Applied | `reader` |
| `unset_clock_transition` | Applied | `reader` |
| `unset_clock_uncertainty` | Applied | `reader` |
| `unset_data_check` | Applied | `sta-si` |
| `unset_disable_clock_gating_check` | Kept | — |
| `unset_disable_timing` | Applied | `sta-si` |
| `unset_input_delay` | Applied | `reader` |
| `unset_output_delay` | Applied | `reader` |
| `unset_path_exceptions` | Applied | `reader` |
| `unset_propagated_clock` | Applied | `reader` |
| `unset_timing_derate` | Applied | `reader` |
| `write_sdc` | Not applied (an output command, not a constraint: listed as not applied) | — |

Options SDC declares that the reference timer does not have. They are read, never refused:

| Command | Option | Status |
| --- | --- | --- |
| `all_inputs` | `-clock` | Kept |
| `all_inputs` | `-level_sensitive` | Kept |
| `all_inputs` | `-edge_triggered` | Kept |
| `all_outputs` | `-clock` | Kept |
| `all_outputs` | `-level_sensitive` | Kept |
| `all_outputs` | `-edge_triggered` | Kept |
| `all_registers` | `-no_hierarchy` | Kept |
| `all_registers` | `-slave_clock_pins` | Kept |
| `all_registers` | `-master_slave` | Kept |
| `current_instance` | `-instance` | Applied |
| `get_ports` | `-hierarchical` | Applied |
| `set_data_check` | `-rise` | Kept |
| `set_data_check` | `-fall` | Kept |
| `set_driving_cell` | `-clock` | Kept |
| `set_driving_cell` | `-clock_fall` | Kept |
| `set_false_path` | `-rise_throough` | Applied |
| `set_false_path` | `-fall_throough` | Applied |
| `set_input_delay` | `-level_sensitive` | Kept |
| `set_load` | `-substract_pin_load` | Applied |
| `set_max_dynamic_power` | `-unit` | Kept |
| `set_max_leakage_power` | `-unit` | Kept |
| `set_operating_conditions` | `-object_list` | Kept |
| `set_output_delay` | `-level_sensitive` | Kept |
| `set_timing_derate` | `-min` | Kept |
| `set_timing_derate` | `-max` | Kept |
| `set_wire_load_selection_group` | `-group_name` | Kept |

Options newer releases of the vendor's SDC parser declare in SDC 1.8, and its public release in no version (†). They are read, never refused:

| Command | Option | Reference timer | Status |
| --- | --- | --- | --- |
| `get_cells` | `-filter` | has it | Applied |
| `get_cells` | `-quiet` | has it | Applied |
| `get_clocks` | `-quiet` | has it | Applied |
| `get_lib_cells` | `-filter` | has it | Applied |
| `get_lib_cells` | `-quiet` | has it | Applied |
| `get_lib_pins` | `-filter` | has it | Applied |
| `get_lib_pins` | `-quiet` | has it | Applied |
| `get_libs` | `-filter` | has it | Applied |
| `get_libs` | `-quiet` | has it | Applied |
| `get_nets` | `-filter` | has it | Applied |
| `get_nets` | `-quiet` | has it | Applied |
| `get_pins` | `-filter` | has it | Applied |
| `get_pins` | `-of_objects` | has it | Applied |
| `get_pins` | `-quiet` | has it | Applied |
| `get_ports` | `-filter` | has it | Applied |
| `get_ports` | `-quiet` | has it | Applied |
| `set_load` | `-rise` | has it | Applied |
| `set_load` | `-fall` | has it | Applied |
| `set_load` | `-internal` | lacks it | Kept |
| `set_load` | `-allow_negative_cap` | lacks it | Kept |
| `set_max_capacitance` | `-clock_path` | lacks it | Kept |
| `set_max_capacitance` | `-data_path` | lacks it | Kept |
| `set_max_capacitance` | `-rise` | lacks it | Kept |
| `set_max_capacitance` | `-fall` | lacks it | Kept |
| `set_max_capacitance` | `-force` | lacks it | Kept |
| `set_max_capacitance` | `-sms_scenarios` | lacks it | Kept |
| `set_max_transition` | `-data_path` | has it | Applied |
| `set_max_transition` | `-delay_penalty` | lacks it | Kept |
| `set_max_transition` | `-force` | lacks it | Kept |

Reference-timer options no SDC version declares. They are read as the reference reads them, and `check` names each one a file uses, at info level (the file is not portable SDC):

| Command | Option |
| --- | --- |
| `all_inputs` | `-no_clocks` |
| `get_clocks` | `-filter` |
| `get_lib_cells` | `-of_objects` |
| `get_lib_pins` | `-of_objects` |
| `get_ports` | `-nocase` |
| `get_ports` | `-of_objects` |
| `group_path` | `-critical_range` |
| `set_clock_sense` | `-clock` |
| `set_driving_cell` | `-cell` |
| `set_false_path` | `-reset_path` |
| `set_max_delay` | `-ignore_clock_latency` |
| `set_max_delay` | `-probe` |
| `set_max_delay` | `-reset_path` |
| `set_min_delay` | `-ignore_clock_latency` |
| `set_min_delay` | `-probe` |
| `set_min_delay` | `-reset_path` |
| `set_multicycle_path` | `-reset_path` |
| `set_port_fanout_number` | `-max` |
| `set_port_fanout_number` | `-min` |
| `set_units` | `-distance` |
<!-- sdc-coverage:end -->

The Tcl, collection and take-back commands such files use are described under Constraints above.

The version dates follow the public Synopsys reference SDC parser (SDC 1.1 to 2.0, the files
above), except `set_sense` (‡) and the commands and options the vendor's releases declare and the
public one does not (†).

### One-edge data checks

`set_data_check -rise` and `-fall` take their SDC meaning: the check applies to that edge of the
data only. Nothing here times a one-edge check, so:

- `check` reports it as a warning, not an error, and the file is still written back whole:
  `` `set_data_check -rise`: kept and written back; it can change timing, and no checker or timer
  here applies it``. With `--fail-on-warning` the exit code is 1.
- The `sta-si` timer skips that check, rather than applying it to both edges, and warns:
  `set_data_check: -rise (a check on one data edge) is not modelled; not applied`. Its other data
  checks are applied.
- The `sta` timer does not model `set_data_check` and refuses a file that has one.

### Clock senses

`set_sense` is the SDC 2.1 command for a sense on a clock or data path, and the replacement for
`set_clock_sense`, per the reference timer's documentation. Both are read, and both are Kept:
written back, and applied by no timer here.

| Option | What it does here |
| --- | --- |
| `-type clock` | A clock sense: read and written back. |
| no `-type` | The same as `-type clock`, the SDC 2.1 default, and written back as `-type clock`. |
| `-type data` | Not modelled: the command is not applied and not written back (a warning). |
| `-positive`, `-negative`, `-stop_propagation` | Read and written back; no timer here applies them. |
| `-clocks` | The clocks the sense is for, written back as `[get_clocks …]`. With `-clock`, `-clocks` wins. |
| `-pulse` | Read and written back; not modelled. |

`check` reports each kind with its count. For `set_sense -stop_propagation [get_pins u1/Q]`:

- `` `set_sense`: kept and written back; it can change timing, and no checker or timer here
  applies it`` (a warning);
- `` `set_sense` no -type: applied as clock (SDC 2.1 default); the reference timer ignores it
  without -type`` (information): a `set_sense` without `-type` is read as clock, as SDC 2.1 says,
  where the reference timer applies nothing for it.

For `set_sense -type data`, a warning: `` `set_sense -type data`: not modelled, not applied; the
reference timer does not apply it either``.

`set_clock_sense` is still accepted, is not reported as deprecated, and is written back as given
(`set_clock_sense`, its options spelled out in full). Its `-clock` option is a reference-timer
extension, reported at information level like the others.

Neither timer applies a clock sense. The `sta` timer refuses a file that has one:
`set_sense / set_clock_sense: not modelled by this timer`. The `sta-si` timer reads the file, does
not apply the sense, and warns once per kind with its count: `` sdc: `set_sense` (2 commands):
kept and written back; not applied by this timer`` (`STA-SDC-NOT-APPLIED`).

### Hierarchy separator

Only `/` is supported. Any other separator stops the read (exit 2), since names written with it
would not resolve. For `set_hierarchy_separator .` the message is:

```text
`set_hierarchy_separator .`: only `/` is supported, and names using another separator would not resolve; the file is not read
```

## Exit status and events

| Exit | Meaning |
| --- | --- |
| 0 | `check`: no errors (with `--fail-on-warning`, no warnings either); `write`: written; `diff`: equivalent |
| 1 | a finding: `check` found an error (or a warning with `--fail-on-warning`), `write` refused, `diff` found a change or a command it could not compare, or `waivers export` left a waiver out |
| 2 | usage, or an input that could not be read |

Every run leaves a `vyges-events` trail on stderr: JSONL when piped, text on a terminal
(`VYGES_LOG_FORMAT=json|text` forces either). Each event names the SDC file as its object:
- one `SDC-<finding>` per check finding, such as `SDC-UNCONSTRAINED-OUTPUT`;
- one `SDC-NOT-WRITTEN` per dropped command (for `diff`, per command not compared);
- one `SDC-DIFF-CHANGE` per change `diff` found, at warn when it loosens or is unknown, else info;
- a summary, `SDC-CHECK`, `SDC-WRITE` or `SDC-DIFF`;
- at debug, `SDC-READ`, which says what the file held.

`-q`/`-v` follow the suite's verbosity contract. They repeat: `-q` warnings, `-qq` errors,
`-qqq` silent, `-v` debug, `-vv` trace. A flag overrides `VYGES_LOG`, which, inherited from
`vyges`, sets the whole command tree.

`vyges-sdc --help` lists every option. `--version` reports the build's commit.

## As a Loom engine

`vyges-sdc --describe` publishes a structured contract. `vyges mcp` then advertises three typed
tools an agent can drive:
- `sdc`, the check, which asserts `clean`;
- `sdc.write`, which asserts `complete` and has the written SDC as its artifact;
- `sdc.diff`, which asserts `equivalent`.

Once installed (`vyges install physical`), they are discoverable.

The checker, the writer and the comparison are the `vyges-loom` implementations (`sdc_lint`,
`Sdc::write` and `sdc_diff`).
The timing engine runs the same code, so `vyges-sdc check` and the timer always agree about what
a file says.

## Licensing

Apache-2.0 (`LICENSE`).
