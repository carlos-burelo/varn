//! SSA optimization passes.
//!
//! Stage 3 adds these one at a time, each independently toggleable and
//! validated against the suite + benchmarks: constant folding, copy
//! propagation, dead-code elimination, CFG simplification, global value
//! numbering, generalized inlining (multi-statement bodies), and escape
//! analysis. Static types drive specialization with no speculation/deopt.

#![allow(dead_code)]
