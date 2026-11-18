//! First-run preparation. Environment probes are read-only and do not launch Agent packages.
pub mod defaults;
pub mod discovery;
pub mod environment;
pub mod initialization;
pub mod model;

#[cfg(test)]
mod tests;
