// SPDX-License-Identifier: Apache-2.0
//! `vyges-sdc` as a library: only its command-line entry, for the suite binaries. The checker
//! and the writer themselves are `vyges_loom::sdc_lint` and `vyges_loom::sdc::Sdc::write`.

/// The `vyges-sdc` command-line entry (the binary's `main`), built into the library too so a
/// suite binary can link every engine and run the one it is invoked as.
#[cfg(feature = "suite")]
#[doc(hidden)]
#[path = "main.rs"]
pub mod cli_main;
