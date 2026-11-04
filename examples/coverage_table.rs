// SPDX-License-Identifier: Apache-2.0
//! Prints the README's SDC command coverage tables, generated from loom's command registry
//! (`vyges_loom::sdc::registry`): `cargo run --example coverage_table`. The README carries the
//! output between its `sdc-coverage` markers, and `tests/cli.rs` fails when the two differ.

fn main() {
    print!("{}", vyges_loom::sdc::registry::coverage_markdown());
}
