//! SSA construction and representation.
//!
//! Stage 2: build a per-function CFG of basic blocks with SSA values and phi
//! nodes (dominance-frontier construction) from HIR, plus a verifier. Opt
//! passes (`crate::passes`) run on this form.

#![allow(dead_code)]
