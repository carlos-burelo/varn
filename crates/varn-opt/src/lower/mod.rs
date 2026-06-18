//! Naive `HIR -> FunctionProto` lowering (Stage 1).
//!
//! Emits the same `varn_types::Chunk`/`FunctionProto` bytecode the existing
//! direct codegen produces — proving HIR completeness + lowering correctness
//! before SSA exists. The register model mirrors the legacy codegen: every
//! identifier read materialises into a fresh temporary, so binary/call dest
//! registers are always temporaries (never live locals). Register layout:
//!   r0          = receiver (always null at the call boundary)
//!   r1..=P      = the P parameters
//!   r(P+1)..    = locals (one fixed slot per HIR local), then temporaries
//!
//! Registers are intentionally not reused across blocks here (correctness over
//! density); `regalloc_post` compresses them downstream.
//!
//! Lowering is split by domain: statements (`stmt`), expressions (`expr`), and
//! class/enum/match values (`class`). This file holds the `FnLower` core
//! (register bookkeeping, `finish`) and the module/function entry points.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, PoolEntry, PolyICSlot};

use crate::hir::*;
use crate::OptError;

mod class;
mod control;
mod expr;
mod stmt;

/// Lower a whole HIR module to the top-level `FunctionProto`. Declared
/// functions are emitted as nested protos and bound to module globals.
pub fn lower_module(module: &HirModule, source_file: Rc<str>, export_names: Vec<Rc<str>>) -> FunctionProto {
    // Lower each declared function to a proto, keyed by name.
    let mut fn_protos: Vec<(Rc<str>, FunctionProto)> = Vec::with_capacity(module.functions.len());
    for f in &module.functions {
        fn_protos.push((f.name.clone(), lower_function(f, source_file.clone())));
    }

    // Reserve fixed slots for the module top level's own locals (block-scoped
    // `let`/`using` at module scope) so temporaries don't collide with them —
    // exactly as `lower_function` does with `f.locals`.
    let mut fl = FnLower::new(0, module.top_level.locals, source_file.clone());
    // Define each declared function as a module global: closure + DefineGlobal.
    for (name, proto) in &fn_protos {
        let proto_idx = fl
            .chunk
            .add_constant(PoolEntry::Function(Rc::new(proto.clone())));
        let dest = fl.alloc();
        fl.chunk
            .write(Chunk::pack_op(OpCode::LoadStaticFn, dest), fl.line);
        fl.chunk.write(proto_idx, fl.line);
        let name_idx = fl.chunk.add_str(name);
        fl.chunk
            .emit_rrc(OpCode::DefineGlobal, 0, dest, name_idx, fl.line);
        fl.free();
    }

    for stmt in &module.top_level.body {
        fl.lower_stmt(stmt);
    }
    fl.finish(Some(Rc::from("<module>")), 0, 0, false, export_names)
}

fn lower_function(f: &HirFunction, source_file: Rc<str>) -> FunctionProto {
    let nparams = f.params.len() as u32;
    let mut fl = FnLower::new(nparams, f.locals, source_file);
    for stmt in &f.body {
        fl.lower_stmt(stmt);
    }
    fl.finish(
        Some(f.name.clone()),
        nparams,
        f.upvalue_count,
        f.has_this,
        Vec::new(),
    )
}

/// How `continue` leaves a loop scope.
#[derive(Clone, Copy)]
enum ContinueMode {
    /// Loop back to a known earlier offset (`while`, `for-of`): the test/header
    /// re-runs. Emitted directly as a `Loop`.
    Backward(usize),
    /// Jump forward to a continue point fixed up after the body (`for-in`'s
    /// increment): collected in `continue_jumps`.
    Forward,
    /// `break`-only scope (`switch`): `continue` skips it to an outer loop.
    Skip,
}

/// A loop or `break`-only (switch) scope on the lowering stack.
struct LoopCtx {
    /// How `continue` exits this scope.
    cont: ContinueMode,
    /// Patch positions of `break` jumps, fixed up at scope end.
    break_jumps: Vec<usize>,
    /// Patch positions of forward `continue` jumps (`ContinueMode::Forward`).
    continue_jumps: Vec<usize>,
    /// Depth of `finally_stack` when the loop opened, so `break`/`continue` run
    /// (and `PopTry`) only the `finally` handlers entered inside the loop.
    finally_depth: usize,
}

struct FnLower {
    chunk: Chunk,
    nparams: u32,
    /// First register available for locals (= 1 + nparams).
    base: u32,
    nlocals: u32,
    /// Bump pointer for temporaries (starts above the local block).
    next_temp: u32,
    /// High-water mark → register_count.
    high: u32,
    line: u32,
    loops: Vec<LoopCtx>,
    /// Pending `finally` bodies (innermost last). `return`/`break`/`continue`
    /// emit `PopTry` + re-lower these before transferring control, mirroring
    /// legacy `Compiler::finally_stack`.
    finally_stack: Vec<Vec<HirStmt>>,
    /// Number of inline-cache slots allocated for this function. Mirrors the
    /// legacy `Compiler::cache_count`; sizes `ic_cache`/`feedback` in `finish`.
    cache_count: u16,
}

impl FnLower {
    fn new(nparams: u32, nlocals: u32, source_file: Rc<str>) -> Self {
        let mut chunk = Chunk::new();
        chunk.source_file = source_file;
        let base = 1 + nparams;
        let temp_start = base + nlocals;
        Self {
            chunk,
            nparams,
            base,
            nlocals,
            next_temp: temp_start,
            high: temp_start.max(1),
            line: 0,
            loops: Vec::new(),
            finally_stack: Vec::new(),
            cache_count: 0,
        }
    }

    /// Reserve an inline-cache slot (used by GetProperty/CallMethod, etc.).
    fn alloc_cache(&mut self) -> u16 {
        let c = self.cache_count;
        self.cache_count += 1;
        c
    }

    /// Emit a property opcode with an inline-cache slot, mirroring legacy
    /// `Compiler::emit_property` (`emit_rrc_ic`).
    fn emit_property(&mut self, op: OpCode, dest: u8, src: u8, name_idx: u16) {
        let cs = self.alloc_cache();
        debug_assert!(cs <= 255, "IC cache slot overflow (>255)");
        self.chunk
            .emit_rrc_ic(op, dest, src, name_idx, cs as u8, self.line);
    }

    fn param_reg(&self, i: u32) -> u8 {
        (1 + i) as u8
    }
    fn local_reg(&self, id: LocalId) -> u8 {
        (self.base + id.0) as u8
    }
    fn alloc(&mut self) -> u8 {
        let r = self.next_temp;
        self.next_temp += 1;
        if self.next_temp > self.high {
            self.high = self.next_temp;
        }
        r as u8
    }
    fn free(&mut self) {
        self.next_temp -= 1;
    }
    fn free_to(&mut self, mark: u32) {
        self.next_temp = mark;
    }

    fn finish(
        self,
        name: Option<Rc<str>>,
        nparams: u32,
        upvalue_count: u32,
        has_this: bool,
        export_names: Vec<Rc<str>>,
    ) -> FunctionProto {
        let cache_count = self.cache_count as usize;
        let mut chunk = self.chunk;
        // Implicit `return null` (matches legacy codegen's epilogue).
        let ret = self.high as u8; // a fresh register above everything used
        let line = self.line;
        chunk.write(Chunk::pack_op(OpCode::LoadNull, ret), line);
        chunk.emit1(OpCode::Return, Chunk::pack(0, ret), line);
        let register_count = (self.high + 1).max(1) as u16;
        FunctionProto {
            name,
            arity: (1 + nparams) as usize,
            export_names,
            register_count,
            has_rest: false,
            is_async: false,
            is_generator: false,
            has_this,
            upvalue_count: upvalue_count as usize,
            cache_count,
            chunk,
            required_caps: Vec::new(),
            register_meta: Vec::new(),
            jit_entry: Cell::new(None),
            jit_code: RefCell::new(None),
            jit_failed: Cell::new(false),
            ic_cache: Rc::new(RefCell::new(
                (0..cache_count).map(|_| PolyICSlot::new()).collect(),
            )),
            feedback: Rc::new(RefCell::new(FeedbackVector::new(cache_count))),
            static_closure_val: Cell::new(0),
        }
    }
}

fn bin_opcode(op: HirBinOp, ty: HirType) -> OpCode {
    use HirBinOp::*;
    match ty {
        HirType::Int => match op {
            Add => OpCode::AddInt,
            Sub => OpCode::SubInt,
            Mul => OpCode::MulInt,
            Div => OpCode::DivInt,
            Mod => OpCode::ModInt,
            Pow => OpCode::PowInt,
            Eq => OpCode::EqInt,
            Ne => OpCode::NeqInt,
            Lt => OpCode::LtInt,
            Le => OpCode::LteInt,
            Gt => OpCode::GtInt,
            Ge => OpCode::GteInt,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add, // unreachable: And/Or are Logical
        },
        HirType::Float => match op {
            Add => OpCode::AddFloat,
            Sub => OpCode::SubFloat,
            Mul => OpCode::MulFloat,
            Div => OpCode::DivFloat,
            Mod => OpCode::ModFloat,
            Pow => OpCode::PowFloat,
            Eq => OpCode::EqFloat,
            Ne => OpCode::NeqFloat,
            Lt => OpCode::LtFloat,
            Le => OpCode::LteFloat,
            Gt => OpCode::GtFloat,
            Ge => OpCode::GteFloat,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add,
        },
        _ => match op {
            Add => OpCode::Add,
            Sub => OpCode::Sub,
            Mul => OpCode::Mul,
            Div => OpCode::Div,
            Mod => OpCode::Mod,
            Pow => OpCode::Pow,
            Eq => OpCode::Eq,
            Ne => OpCode::Neq,
            Lt => OpCode::Lt,
            Le => OpCode::Lte,
            Gt => OpCode::Gt,
            Ge => OpCode::Gte,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
            And | Or => OpCode::Add,
        },
    }
}

/// Entry used by `crate::compile`.
pub fn lower(module: &HirModule, source_file: Rc<str>, export_names: Vec<Rc<str>>) -> Result<FunctionProto, OptError> {
    Ok(lower_module(module, source_file, export_names))
}
