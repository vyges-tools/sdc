# vyges-sdc

A **Loom engine** for **SDC timing constraints**: it checks them, and it writes them.

A timer computes timing on whatever constraints it is given. A constraint that is wrong does not
produce a wrong-looking report; it produces a **clean** one. An output with no
`set_output_delay` is a path nobody checks. A clock on a port that drives nothing leaves every
register untimed while the SDC still looks populated. A `set_false_path` with a typo names
nothing and quietly does nothing. `vyges-sdc check` looks for exactly these.

## Usage

```sh
vyges loom sdc check --sdc design.sdc --netlist design.v --lib cells.lib [--metadata vyges-metadata.json] [--json]
vyges loom sdc write --sdc design.sdc -o normalised.sdc [--netlist design.v --lib cells.lib] [--partial] [--json]
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

`-q` and `-v` move the level floor (`VYGES_LOG`); an explicit `VYGES_LOG` wins.

`vyges-sdc --help` lists every option. `--version` reports the build's commit.

## As a Loom engine

`vyges-sdc --describe` publishes a structured contract. `vyges mcp` then advertises two typed
tools an agent can drive:
- `sdc`, the check, which asserts `clean`;
- `sdc.write`, which asserts `complete` and has the written SDC as its artifact.

Once installed (`vyges install loom`), they are discoverable.

The checker and the writer are the `vyges-loom` implementations (`sdc_lint` and `Sdc::write`).
The timing engine runs the same code, so `vyges-sdc check` and the timer always agree about what
a file says.

## Licensing

Apache-2.0 (`LICENSE`).
