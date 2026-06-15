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

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use varn_core::OpCode;
use varn_types::chunk::{Chunk, FeedbackVector, FunctionProto, Literal, PoolEntry, PolyICSlot};

use crate::hir::*;
use crate::OptError;

/// Lower a whole HIR module to the top-level `FunctionProto`. Declared
/// functions are emitted as nested protos and bound to module globals.
pub fn lower_module(module: &HirModule, source_file: Rc<str>, export_names: Vec<Rc<str>>) -> FunctionProto {
    // Lower each declared function to a proto, keyed by name.
    let mut fn_protos: Vec<(Rc<str>, FunctionProto)> = Vec::with_capacity(module.functions.len());
    for f in &module.functions {
        fn_protos.push((f.name.clone(), lower_function(f, source_file.clone())));
    }

    let mut fl = FnLower::new(0, 0, source_file.clone());
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
    fl.finish(Some(Rc::from("<module>")), 0, export_names)
}

fn lower_function(f: &HirFunction, source_file: Rc<str>) -> FunctionProto {
    let nparams = f.params.len() as u32;
    let mut fl = FnLower::new(nparams, f.locals, source_file);
    for stmt in &f.body {
        fl.lower_stmt(stmt);
    }
    fl.finish(Some(f.name.clone()), nparams, Vec::new())
}

struct LoopCtx {
    /// Code offset to `Loop` back to on `continue` (the loop's test).
    continue_target: usize,
    /// Patch positions of `break` jumps, fixed up at loop end.
    break_jumps: Vec<usize>,
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
        }
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

    fn finish(self, name: Option<Rc<str>>, nparams: u32, export_names: Vec<Rc<str>>) -> FunctionProto {
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
            has_this: false,
            upvalue_count: 0,
            cache_count: 0,
            chunk,
            required_caps: Vec::new(),
            register_meta: Vec::new(),
            jit_entry: Cell::new(None),
            jit_code: RefCell::new(None),
            jit_failed: Cell::new(false),
            ic_cache: Rc::new(RefCell::new(Vec::<PolyICSlot>::new())),
            feedback: Rc::new(RefCell::new(FeedbackVector::new(0))),
            static_closure_val: Cell::new(0),
        }
    }

    fn lower_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Expr(e) => {
                let mark = self.next_temp;
                let _ = self.lower_expr(e);
                self.free_to(mark);
            }
            HirStmt::Let { local, value, .. } => {
                let mark = self.next_temp;
                let v = self.lower_expr(value);
                let dst = self.local_reg(*local);
                self.chunk.emit_rr(OpCode::Move, dst, v, self.line);
                self.free_to(mark);
            }
            HirStmt::Assign { target, value } => {
                let mark = self.next_temp;
                let v = self.lower_expr(value);
                self.store_binding(target, v);
                self.free_to(mark);
            }
            HirStmt::Return(v) => {
                let mark = self.next_temp;
                let r = match v {
                    Some(e) => self.lower_expr(e),
                    None => {
                        let r = self.alloc();
                        self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                        r
                    }
                };
                self.chunk
                    .emit1(OpCode::Return, Chunk::pack(0, r), self.line);
                self.free_to(mark);
            }
            HirStmt::If {
                test,
                then_body,
                else_body,
            } => {
                let mark = self.next_temp;
                let cond = self.lower_expr(test);
                let else_j = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
                self.free_to(mark);
                for s in then_body {
                    self.lower_stmt(s);
                }
                if else_body.is_empty() {
                    self.chunk.patch_jump(else_j);
                } else {
                    let end_j = self.chunk.emit_jump(OpCode::Jump, self.line);
                    self.chunk.patch_jump(else_j);
                    for s in else_body {
                        self.lower_stmt(s);
                    }
                    self.chunk.patch_jump(end_j);
                }
            }
            HirStmt::While { test, body } => {
                let loop_start = self.chunk.code.len();
                let mark = self.next_temp;
                let cond = self.lower_expr(test);
                let exit = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
                self.free_to(mark);
                self.loops.push(LoopCtx {
                    continue_target: loop_start,
                    break_jumps: Vec::new(),
                });
                for s in body {
                    self.lower_stmt(s);
                }
                self.chunk.emit_loop(loop_start, self.line);
                self.chunk.patch_jump(exit);
                let ctx = self.loops.pop().unwrap();
                for bj in ctx.break_jumps {
                    self.chunk.patch_jump(bj);
                }
            }
            HirStmt::Break => {
                let j = self.chunk.emit_jump(OpCode::Jump, self.line);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.break_jumps.push(j);
                }
            }
            HirStmt::Continue => {
                if let Some(ctx) = self.loops.last() {
                    let target = ctx.continue_target;
                    self.chunk.emit_loop(target, self.line);
                }
            }
        }
    }

    fn store_binding(&mut self, target: &HirBinding, value_reg: u8) {
        match target {
            HirBinding::Param(i) => {
                let dst = self.param_reg(*i);
                self.chunk.emit_rr(OpCode::Move, dst, value_reg, self.line);
            }
            HirBinding::Local(id) => {
                let dst = self.local_reg(*id);
                self.chunk.emit_rr(OpCode::Move, dst, value_reg, self.line);
            }
            HirBinding::Global(name) => {
                let idx = self.chunk.add_str(name);
                self.chunk
                    .emit_rrc(OpCode::DefineGlobal, 0, value_reg, idx, self.line);
            }
            HirBinding::Upvalue(_) => unreachable!("upvalues are unsupported in core lowering"),
        }
    }

    /// Lower an expression, returning the register holding its value.
    fn lower_expr(&mut self, expr: &HirExpr) -> u8 {
        match expr {
            HirExpr::Int(n) => {
                let r = self.alloc();
                self.chunk.emit_load_int(r, *n, self.line);
                r
            }
            HirExpr::Float(f) => {
                let idx = self
                    .chunk
                    .add_constant(PoolEntry::Literal(Literal::Float(*f)));
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::Str(s) => {
                let idx = self.chunk.add_str(s);
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::Bool(b) => {
                let r = self.alloc();
                let op = if *b { OpCode::LoadTrue } else { OpCode::LoadFalse };
                self.chunk.emit_rr(op, r, 0, self.line);
                r
            }
            HirExpr::Null => {
                let r = self.alloc();
                self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                r
            }
            HirExpr::Var(binding) => self.load_binding(binding),
            HirExpr::Binary { op, lhs, rhs, ty } => {
                let l = self.lower_expr(lhs);
                let r = self.lower_expr(rhs);
                let opcode = bin_opcode(*op, *ty);
                self.chunk.emit_rrr(opcode, l, l, r, self.line);
                self.free(); // free r, result stays in l
                l
            }
            HirExpr::Unary { op, operand, .. } => {
                let s = self.lower_expr(operand);
                let opcode = match op {
                    HirUnOp::Neg => OpCode::Negate,
                    HirUnOp::Not => OpCode::Not,
                };
                self.chunk.emit_rr(opcode, s, s, self.line);
                s
            }
            HirExpr::Call { callee, args, .. } => self.lower_call(callee, args),
            HirExpr::Member { .. } | HirExpr::Index { .. } => {
                unreachable!("member/index unsupported in core lowering")
            }
        }
    }

    fn load_binding(&mut self, binding: &HirBinding) -> u8 {
        let r = self.alloc();
        match binding {
            HirBinding::Param(i) => {
                let src = self.param_reg(*i);
                self.chunk.emit_rr(OpCode::Move, r, src, self.line);
            }
            HirBinding::Local(id) => {
                let src = self.local_reg(*id);
                self.chunk.emit_rr(OpCode::Move, r, src, self.line);
            }
            HirBinding::Global(name) => {
                let idx = self.chunk.add_str(name);
                self.chunk.emit_rc(OpCode::LoadGlobal, r, idx, self.line);
            }
            HirBinding::Upvalue(_) => unreachable!("upvalues unsupported in core lowering"),
        }
        r
    }

    /// Plain call ABI: `LoadNull` receiver, contiguous args, `Call dest=callee`.
    fn lower_call(&mut self, callee: &HirExpr, args: &[HirExpr]) -> u8 {
        let callee_reg = self.lower_expr(callee);
        let recv = self.alloc();
        self.chunk.emit_rr(OpCode::LoadNull, recv, 0, self.line);
        let arg_start = recv + 1;
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            let r = self.lower_expr(a);
            if r != slot {
                while (self.next_temp as u8) <= slot {
                    self.alloc();
                }
                self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
            }
        }
        let total = (args.len() + 1) as u8; // receiver + args
        self.chunk.emit(OpCode::Call, self.line);
        self.chunk
            .write(Chunk::pack(callee_reg, callee_reg), self.line);
        self.chunk.write(Chunk::pack(total, recv), self.line);
        // Result lands in dest = callee_reg; free everything above it.
        self.free_to(callee_reg as u32 + 1);
        callee_reg
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
            And | Or => OpCode::Add,
        },
    }
}

/// Entry used by `crate::compile`.
pub fn lower(module: &HirModule, source_file: Rc<str>, export_names: Vec<Rc<str>>) -> Result<FunctionProto, OptError> {
    Ok(lower_module(module, source_file, export_names))
}
