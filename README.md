# vyges-sdc

A **Loom engine** for **SDC timing constraints**: it checks them, and it writes them.

A timer computes timing on whatever constraints it is given. A constraint that is wrong does not
produce a wrong-looking report; it produces a **clean** one. An output with no
`set_output_delay` is a path nobody checks. A clock on a port that drives nothing leaves every
register untimed while the SDC still looks populated. A `set_false_path` with a typo names
nothing and quietly does nothing. `vyges-sdc check` looks for exactly these.

## Usage

```sh
vyges physical sdc check --sdc design.sdc [--netlist design.v --lib cells.lib] [--metadata vyges-metadata.json] [--define NAME=VALUE ...] [--json]
vyges physical sdc write --sdc design.sdc -o normalised.sdc [--netlist design.v --lib cells.lib] [--define NAME=VALUE ...] [--partial] [--json]
```

### `check`

`check` tests the constraints against the netlist, with the Liberty to say which cells are
registers:

- **Completeness:** inputs and outputs with no I/O delay, registers that no clock reaches, and
  endpoint coverage (the share of timing endpoints that are constrained at all), reported on
  every run, clean ones included.
- **Consistency:** duplicate clock names, clocks on ports the design does not have, and
  exceptions (`set_false_path`, `set_multicycle_path`) that name no object or no structural path.
- **Declared clock domains:** given `--metadata`, the IP's `clock_domains` from
  `vyges-metadata.json` are checked against the SDC's clocks. A three-clock IP with one
  `create_clock` passes timing and times only part of the design; this catches it.

Every check also reports each command that was read but **not applied**: one the model does not
carry, one it refused, or one whose object query it could not evaluate. Such a command is an
error when it affects timing (a constraint the timer never sees is a hole in the constraints) and
information when it does not (a synthesis directive).

**Without `--netlist`**, `check` runs what the file alone supports: the not-applied report, clock
periods and duplicate names, plus one note that the design checks did not run.

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
  "not_written": [ "not modelled: set_disable_timing" ] }
```

Malformed constraints are refused when the file is read rather than guessed at. For example, a
`-period` given with a unit (`20ns`, `"1 MHz"`) is an error, because SDC periods are plain
numbers in the file's time unit.

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
`[remove_from_collection [all_inputs] clk]`.

**Tcl.** The subset constraint files use: `set` and variables (`$name`, `${name}`,
`$::env(NAME)`), `if`/`elseif`/`else`, `foreach` over a list, `switch`, `expr`, `proc` (each call
runs its body with its parameters bound and its own local variables; `return` ends it, and a proc
called inside `[…]` stands for the value it returns), `global`, `upvar` (the caller's variable,
written back when the proc returns), `dict` (`create`, `set`, `get`, `exists`, `keys`, `values`,
`size`, `merge`, `unset`, `lappend`, `append`, `incr` and `for`, in insertion order), `source`
(read in place, from the working directory and then from the reading file's directory; a `return`
at a file's top level ends that file), `puts`, and abbreviated command and option names, expanded
as the reference expands them. Nothing is substituted inside braces, as in Tcl. The singular
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

**Everything else** the reference reads is read too. Constraints no timer here applies yet
(`set_data_check`, `group_path`, `set_min_pulse_width`, `set_max_time_borrow`, the wire-load and
operating-condition commands, for example) are kept and written back as they were read. A command
the reference runs but that is no constraint (a tool setting such as `set_assigned_delay`) is
reported by name: `check` reports it, and `write` lists it as not written. A command that is not SDC or Tcl at all stops the read with an error that names it and
the line it is on (exit 2). The reference timer stops there too, and a constraint file read only
partly would otherwise be timed as if it were whole.

## Exit status and events

| Exit | Meaning |
| --- | --- |
| 0 | `check`: no errors (with `--fail-on-warning`, no warnings either); `write`: written |
| 1 | a finding: `check` found an error (or a warning with `--fail-on-warning`), or `write` refused |
| 2 | usage, or an input that could not be read |

Every run leaves a `vyges-events` trail on stderr: JSONL when piped, text on a terminal
(`VYGES_LOG_FORMAT=json|text` forces either). Each event names the SDC file as its object:
- one `SDC-<finding>` per check finding, such as `SDC-UNCONSTRAINED-OUTPUT`;
- one `SDC-NOT-WRITTEN` per dropped command;
- a summary, `SDC-CHECK` or `SDC-WRITE`;
- at debug, `SDC-READ`, which says what the file held.

`-q`/`-v` follow the suite's verbosity contract. They repeat: `-q` warnings, `-qq` errors,
`-qqq` silent, `-v` debug, `-vv` trace. A flag overrides `VYGES_LOG`, which, inherited from
`vyges`, sets the whole command tree.

`vyges-sdc --help` lists every option. `--version` reports the build's commit.

## As a Loom engine

`vyges-sdc --describe` publishes a structured contract. `vyges mcp` then advertises two typed
tools an agent can drive:
- `sdc`, the check, which asserts `clean`;
- `sdc.write`, which asserts `complete` and has the written SDC as its artifact.

Once installed (`vyges install physical`), they are discoverable.

The checker and the writer are the `vyges-loom` implementations (`sdc_lint` and `Sdc::write`).
The timing engine runs the same code, so `vyges-sdc check` and the timer always agree about what
a file says.

## Licensing

Apache-2.0 (`LICENSE`).
