// Compile the production memory module and its private fault-injection tests in
// a small harness, without code-generating the monolithic library unit suite.
pub use gold_band::{config, provider, storage};

#[allow(dead_code)]
#[path = "../src/channel.rs"]
mod channel;

#[allow(dead_code)]
#[path = "../src/memory/mod.rs"]
mod memory;
