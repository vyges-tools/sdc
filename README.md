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

It reports, too, each command that is **kept** (read and written back) but applied by no checker
or timer here, with its count: `set_clock_gating_check`, the wire-load and operating-condition
commands, `set_sense`, for example (the coverage table below marks them Kept). That is a warning
when applying it could move a slack, so the report is not called clean, and information when it
could not (an area or power directive). It is not an error, since the command was read as the
reference timer reads it. The JSON result counts them in `kept_not_applied`.

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

**SDC options the reference timer lacks** are read, never refused: `-level_sensitive` on
`set_input_delay` / `set_output_delay`, `-min` / `-max` on `set_timing_derate`, `-clock` /
`-clock_fall` on `set_driving_cell`, the `all_inputs`, `all_outputs` and `all_registers` options,
`-unit` on the power limits, `-object_list` on `set_operating_conditions` and `-group_name` on
`set_wire_load_selection_group`. Each is written back, and `check` reports it as kept but not
applied: a command whose meaning it changes (a clock-scoped drive, an input list selected by
clock) is kept as written rather than applied as if the option were not there. `get_ports
-hierarchical` is `get_ports` (a port is at the top level), and `set_load -substract_pin_load` is
`-subtract_pin_load`. The table at the end of the command coverage lists them.

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

**Everything else** the reference reads is read too. Constraints no checker or timer here applies
yet (`group_path`, `set_min_pulse_width`, `set_max_time_borrow`, the wire-load and
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
1.1. ‡ No public source declares SDC 2.1, so `set_sense` is dated 2.1 without one to check it
against; `set_clock_jitter` and `set_disable_clock_gating_check` are declared by no version the
parser covers, and are listed outside the set. Option-level detail follows the SDC command descriptions; the table is
generated from a one-line probe per command and option, run through this engine and through the
reference timer, so it says only what the code does.

- **Applied**: read into the constraint model, written back, and applied by the tool named under
  "Applied by": `check` (this engine's check), `sta` (the `vyges-tools/sta` timer), `sta-si` (the
  `vyges-tools/sta-si` timer), `rsz` (the `vyges-tools/rsz` design repair, for the design-rule
  limits), or `reader` (evaluated while the file is read: object queries, units, the current
  design and instance, and take-back commands; what it changes is applied where the command it
  changes is).
- **Kept**: read as the reference timer reads it and written back, but applied by no checker or
  timer here. `check` reports each one it reads, with its count.
- **Tcl**: run as Tcl runs it.
- **Read past**: recognised and skipped (it steers synthesis or power, not timing); `write` lists it
  as not written, and `--partial` writes the rest.
- **Unhandled**: not read; the file stops there (exit 2), as it does in the reference timer.

<!-- sdc-coverage:begin -->
SDC 1.1 to 2.1: 51 applied, 26 kept, 3 Tcl, 1 read past, 0 unhandled, of 81 commands.

| Command | SDC | Status | Applied by |
| --- | --- | --- | --- |
| `all_clocks` | 1.1 | Applied | `reader` |
| `all_inputs` | 1.1 | Applied | `reader` |
| `all_outputs` | 1.1 | Applied | `reader` |
| `all_registers` | 1.7 | Applied | `reader` |
| `create_clock` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `create_generated_clock` | 1.3 | Applied | `check`, `sta`, `sta-si` |
| `create_voltage_area` | 1.6 | Kept | — |
| `current_design` | 1.1 | Applied | `reader` |
| `current_instance` | 1.1 | Applied | `reader` |
| `expr` | Tcl | Tcl | — |
| `get_cell` | 1.5 | Applied (spelling of `get_cells`) | `reader` |
| `get_cells` | 1.1 | Applied | `reader` |
| `get_clock` | 1.5 | Applied (spelling of `get_clocks`) | `reader` |
| `get_clocks` | 1.1 | Applied | `reader` |
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
| `group_path` | 1.7 | Kept | — |
| `list` | Tcl | Tcl | — |
| `set` | Tcl | Tcl | — |
| `set_case_analysis` | 1.1 | Applied | `sta`, `sta-si` |
| `set_clock_gating_check` | 1.2 | Kept | — |
| `set_clock_group` | 1.7 | Applied (spelling of `set_clock_groups`) | `sta`, `sta-si` |
| `set_clock_groups` | 1.7 | Applied | `sta`, `sta-si` |
| `set_clock_latency` | 1.1 | Applied | `sta`, `sta-si` |
| `set_clock_sense` | 1.7 | Kept | — |
| `set_clock_transition` | 1.1 | Applied | `sta`, `sta-si` |
| `set_clock_uncertainty` | 1.1 | Applied | `sta`, `sta-si` |
| `set_data_check` | 1.4 | Applied | `sta-si` |
| `set_disable_timing` | 1.1 | Applied | `sta-si` |
| `set_drive` | 1.1 | Kept | — |
| `set_driving_cell` | 1.1 | Applied | `sta`, `sta-si` |
| `set_false_path` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_fanout_load` | 1.1 | Kept | — |
| `set_hierarchy_separator` | 1.2 | Applied | `reader` |
| `set_ideal_latency` | 1.7 | Kept | — |
| `set_ideal_network` | 1.7 | Kept | — |
| `set_ideal_transition` | 1.7 | Kept | — |
| `set_input_delay` | 1.1 | Applied | `check`, `sta`, `sta-si` |
| `set_input_transition` | 1.1 | Applied | `sta`, `sta-si` |
| `set_level_shifter_strategy` | 1.6 | Kept | — |
| `set_level_shifter_threshold` | 1.6 | Kept | — |
| `set_load` | 1.1 | Applied | `sta`, `sta-si` |
| `set_logic_dc` | 1.1 | Applied | `sta`, `sta-si` |
| `set_logic_one` | 1.1 | Applied | `sta`, `sta-si` |
| `set_logic_zero` | 1.1 | Applied | `sta`, `sta-si` |
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
| `set_sense` | 2.1 ‡ | Kept | — |
| `set_timing_derate` | 1.5 | Applied | `sta`, `sta-si` |
| `set_unit` | 1.7 | Applied (spelling of `set_units`) | `reader` |
| `set_units` | 1.7 | Applied | `reader` |
| `set_voltage` | 1.8 | Kept | — |
| `set_wire_load_min_block_size` | 1.1 | Kept | — |
| `set_wire_load_mode` | 1.1 | Kept | — |
| `set_wire_load_model` | 1.1 | Kept | — |
| `set_wire_load_selection_group` | 1.1 | Kept | — |

Commands outside the SDC versions the reference parser declares, found in constraint files written for other tools:

| Command | Status | Applied by |
| --- | --- | --- |
| `remove_propagated_clock` | Applied | `reader` |
| `set_clock_jitter` | Unhandled | — |
| `set_current_design` | Unhandled | — |
| `set_disable_clock_gating_check` | Kept | — |
| `set_dont_touch` | Read past | — |
| `set_path_margin` | Kept | — |
| `set_pvt` | Kept | — |
| `unset_data_check` | Applied | `sta-si` |
| `unset_disable_clock_gating_check` | Kept | — |
| `unset_disable_timing` | Applied | `sta-si` |
| `unset_propagated_clock` | Applied | `reader` |

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
| `get_ports` | `-hierarchical` | Applied |
| `set_driving_cell` | `-clock` | Kept |
| `set_driving_cell` | `-clock_fall` | Kept |
| `set_input_delay` | `-level_sensitive` | Kept |
| `set_load` | `-substract_pin_load` | Applied |
| `set_max_dynamic_power` | `-unit` | Kept |
| `set_max_leakage_power` | `-unit` | Kept |
| `set_operating_conditions` | `-object_list` | Kept |
| `set_output_delay` | `-level_sensitive` | Kept |
| `set_timing_derate` | `-min` | Kept |
| `set_timing_derate` | `-max` | Kept |
| `set_wire_load_selection_group` | `-group_name` | Kept |
<!-- sdc-coverage:end -->

The Tcl, collection and take-back commands such files use are described under Constraints above.

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
