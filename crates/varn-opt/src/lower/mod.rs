//! Lowering to bytecode.
//!
//! Two entry points will live here:
//! - a naive `HIR -> FunctionProto` path used during Stage 1 bring-up (proves
//!   HIR completeness + lowering correctness before SSA exists), and
//! - the real `SSA -> FunctionProto` out-of-SSA + instruction-selection path.
//!
//! Both emit the same `varn_types::Chunk`/`FunctionProto` the existing
//! register VM and JIT consume; `regalloc_post` then runs as today.

#![allow(dead_code)]
