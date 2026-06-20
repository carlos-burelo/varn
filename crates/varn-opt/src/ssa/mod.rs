//! SSA construction and representation.
//!
//! Stage 2: build a per-function CFG of basic blocks in SSA form from HIR, run
//! opt passes (`crate::passes`) on it, then lower back to bytecode. The
//! representation uses **block parameters** instead of phi nodes and is built
//! on-the-fly (Braun et al. 2013); a dominator tree is computed separately for
//! the verifier and passes that need it.

#![allow(dead_code)]

pub mod build;
pub mod dump;
pub mod ir;

#[cfg(test)]
mod tests;
