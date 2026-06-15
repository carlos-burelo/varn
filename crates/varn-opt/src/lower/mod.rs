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
            HirStmt::SetMember {
                object,
                name,
                value,
            } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let val = self.lower_expr(value);
                let key_idx = self.chunk.add_str(name);
                // SetProperty obj.key = val (with an IC slot).
                self.emit_property(OpCode::SetProperty, obj, val, key_idx);
                self.free_to(mark);
            }
            HirStmt::SetIndex {
                object,
                index,
                value,
            } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let idx = self.lower_expr(index);
                let val = self.lower_expr(value);
                self.chunk
                    .emit_rrr(OpCode::SetIndex, obj, idx, val, self.line);
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
            HirStmt::Import {
                source,
                is_type,
                specs,
            } => {
                let mark = self.next_temp;
                let src_idx = self.chunk.add_str(source);
                let mod_reg = self.alloc();
                self.chunk
                    .emit_rc(OpCode::LoadModule, mod_reg, src_idx, self.line);
                if !*is_type {
                    for spec in specs {
                        match &spec.kind {
                            HirImportKind::Namespace => {
                                let local_idx = self.chunk.add_str(&spec.local);
                                self.chunk.emit_rrc(
                                    OpCode::DefineGlobal,
                                    0,
                                    mod_reg,
                                    local_idx,
                                    self.line,
                                );
                            }
                            HirImportKind::Default | HirImportKind::Named(_) => {
                                let dest = self.alloc();
                                if let Some(slot) = spec.slot {
                                    self.chunk.emit_rrc(
                                        OpCode::LoadModuleSlot,
                                        dest,
                                        mod_reg,
                                        slot,
                                        self.line,
                                    );
                                } else {
                                    let key = match &spec.kind {
                                        HirImportKind::Named(n) => n.as_ref(),
                                        _ => "default",
                                    };
                                    let key_idx = self.chunk.add_str(key);
                                    self.emit_property(OpCode::GetProperty, dest, mod_reg, key_idx);
                                }
                                let local_idx = self.chunk.add_str(&spec.local);
                                self.chunk.emit_rrc(
                                    OpCode::DefineGlobal,
                                    0,
                                    dest,
                                    local_idx,
                                    self.line,
                                );
                                self.free_to(mod_reg as u32 + 1);
                            }
                        }
                    }
                }
                self.free_to(mark);
            }
            HirStmt::StoreExport { name, slot } => {
                let mark = self.next_temp;
                let r = self.alloc();
                let idx = self.chunk.add_str(name);
                self.chunk.emit_rc(OpCode::LoadGlobal, r, idx, self.line);
                self.chunk
                    .emit_rc(OpCode::StoreModuleSlot, r, *slot, self.line);
                self.free_to(mark);
            }
            HirStmt::CloseUpvalues(targets) => {
                // Close open upvalues over the lowest captured slot, matching
                // legacy `pop_scope`. The VM closes everything at or above it.
                let lowest = targets
                    .iter()
                    .map(|t| match t {
                        CaptureTarget::Param(i) => self.param_reg(*i),
                        CaptureTarget::Local(id) => self.local_reg(*id),
                    })
                    .min()
                    .unwrap_or(0);
                self.chunk
                    .emit1(OpCode::CloseUpvalue, lowest as u16, self.line);
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
            HirBinding::Upvalue(uv) => {
                // StoreUpvalue: uv in hi byte, src in lo (legacy `emit_store_var`).
                self.chunk.emit1(
                    OpCode::StoreUpvalue,
                    Chunk::pack(*uv as u8, value_reg),
                    self.line,
                );
            }
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
            HirExpr::SelfCall { args, .. } => self.lower_self_call(args),
            HirExpr::MethodCall {
                recv, name, args, ..
            } => self.lower_method_call(recv, name, args),
            HirExpr::Member { object, name, .. } => {
                // `object.name` → GetProperty with an inline-cache slot.
                let obj = self.lower_expr(object);
                let name_idx = self.chunk.add_str(name);
                self.emit_property(OpCode::GetProperty, obj, obj, name_idx);
                obj
            }
            HirExpr::Index { object, index, .. } => {
                // `object[index]` → GetIndex (no IC).
                let obj = self.lower_expr(object);
                let idx = self.lower_expr(index);
                self.chunk.emit_rrr(OpCode::GetIndex, obj, obj, idx, self.line);
                self.free(); // free idx; result stays in obj
                obj
            }
            HirExpr::Logical { op, lhs, rhs } => self.lower_logical(*op, lhs, rhs),
            HirExpr::Conditional { test, cons, alt } => self.lower_conditional(test, cons, alt),
            HirExpr::Update { target, op, prefix } => self.lower_update(target, *op, *prefix),
            HirExpr::Array(elements) => self.lower_array(elements),
            HirExpr::Object { keys, values } => self.lower_object(keys, values),
            HirExpr::Closure { func, upvalues } => self.lower_closure(func, upvalues),
            HirExpr::This => {
                // The receiver lives in register 0; copy it into a temp.
                let r = self.alloc();
                self.chunk.emit_rr(OpCode::Move, r, 0, self.line);
                r
            }
            HirExpr::Class(cls) => self.lower_class_value(cls),
            HirExpr::Enum(en) => self.lower_enum_value(en),
            HirExpr::Match { subject, cases } => self.lower_match_value(subject, cases),
        }
    }

    /// Build an enum value: `MakeClass`, then per variant a `MakeEnumVariant`
    /// bound as a `DefineStatic`, then instance fields/methods. Mirrors legacy
    /// `compile_enum_expr`.
    fn lower_enum_value(&mut self, en: &HirEnum) -> u8 {
        let name_idx = self.chunk.add_str(&en.name);
        let class_reg = self.alloc();
        self.chunk
            .emit_rrc(OpCode::MakeClass, class_reg, 0, name_idx, self.line);
        for v in &en.variants {
            let mark = self.next_temp;
            let tag_reg = self.alloc();
            self.chunk.emit_load_int(tag_reg, v.tag, self.line);
            let variant_reg = self.alloc();
            let meta_idx = self.chunk.add_str(&v.meta);
            self.chunk.emit(OpCode::MakeEnumVariant, self.line);
            self.chunk
                .write(Chunk::pack(variant_reg, tag_reg), self.line);
            self.chunk.write(meta_idx, self.line);
            let key_idx = self.chunk.add_str(&v.name);
            self.chunk.emit(OpCode::DefineStatic, self.line);
            self.chunk
                .write(Chunk::pack(class_reg, variant_reg), self.line);
            self.chunk.write(key_idx, self.line);
            self.free_to(mark);
        }
        for field in &en.fields {
            let key_idx = self.chunk.add_str(field);
            self.chunk.emit(OpCode::DeclareField, self.line);
            self.chunk.write(Chunk::pack(class_reg, 0), self.line);
            self.chunk.write(key_idx, self.line);
        }
        for m in &en.methods {
            self.bind_method(class_reg, m);
        }
        class_reg
    }

    /// `match` → a branch chain. `subj` and `dest` sit at the two lowest temps;
    /// each case tests/binds, runs its body, and `Move`s its result into `dest`.
    /// Mirrors legacy `compile_match`.
    fn lower_match_value(&mut self, subject: &HirExpr, cases: &[HirMatchCase]) -> u8 {
        let subj = self.lower_expr(subject);
        let dest = self.alloc();
        self.chunk.emit_rr(OpCode::LoadNull, dest, 0, self.line);
        let mut end_jumps: Vec<usize> = Vec::new();
        for case in cases {
            let mark = self.next_temp;
            let skip = self.lower_case_test(&case.test, subj);
            for stmt in &case.body {
                self.lower_stmt(stmt);
            }
            let body_r = match &case.result {
                Some(e) => self.lower_expr(e),
                None => {
                    let r = self.alloc();
                    self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                    r
                }
            };
            self.chunk.emit_rr(OpCode::Move, dest, body_r, self.line);
            let end = self.chunk.emit_jump(OpCode::Jump, self.line);
            end_jumps.push(end);
            if let Some(s) = skip {
                self.chunk.patch_jump(s);
            }
            self.free_to(mark);
        }
        for j in end_jumps {
            self.chunk.patch_jump(j);
        }
        // Move the result down into the subject slot so the value lands at the
        // entry temp (the `lower_expr` net-`+1` contract).
        self.chunk.emit_rr(OpCode::Move, subj, dest, self.line);
        self.free_to(subj as u32 + 1);
        subj
    }

    /// Emit a match case's pattern test + bindings; returns the skip jump to
    /// patch to the next case (`None` for always-matching patterns).
    fn lower_case_test(&mut self, test: &HirCaseTest, subj: u8) -> Option<usize> {
        match test {
            HirCaseTest::Wildcard => None,
            HirCaseTest::Literal(lit) => {
                let lit_r = self.lower_expr(lit);
                let eq = self.alloc();
                self.chunk.emit_rrr(OpCode::Eq, eq, subj, lit_r, self.line);
                Some(self.chunk.emit_cond_jump(OpCode::JumpIfFalse, eq, self.line))
            }
            HirCaseTest::Bind(local) => {
                let dst = self.local_reg(*local);
                self.chunk.emit_rr(OpCode::Move, dst, subj, self.line);
                None
            }
            HirCaseTest::EnumVariant { name, binds } => {
                let tag_key = self.chunk.add_str("__variant_name__");
                let tag_r = self.alloc();
                self.emit_property(OpCode::GetProperty, tag_r, subj, tag_key);
                let vname_idx = self.chunk.add_str(name);
                let vname_r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, vname_r, vname_idx, self.line);
                let eq = self.alloc();
                self.chunk.emit_rrr(OpCode::Eq, eq, tag_r, vname_r, self.line);
                let j = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, eq, self.line);
                for (i, b) in binds.iter().enumerate() {
                    if let Some(local) = b {
                        let fkey = self.chunk.add_str(format!("value{i}"));
                        let field_r = self.alloc();
                        self.emit_property(OpCode::GetProperty, field_r, subj, fkey);
                        let dst = self.local_reg(*local);
                        self.chunk.emit_rr(OpCode::Move, dst, field_r, self.line);
                    }
                }
                Some(j)
            }
        }
    }

    /// Build a class value: `MakeClass` + `DeclareField`(s) + `Method`(s).
    fn lower_class_value(&mut self, cls: &HirClass) -> u8 {
        let name_idx = self.chunk.add_str(&cls.name);
        let class_reg = self.alloc();
        self.chunk
            .emit_rrc(OpCode::MakeClass, class_reg, 0, name_idx, self.line);
        for field in &cls.fields {
            let key_idx = self.chunk.add_str(field);
            self.chunk.emit(OpCode::DeclareField, self.line);
            self.chunk.write(Chunk::pack(class_reg, 0), self.line);
            self.chunk.write(key_idx, self.line);
        }
        self.bind_method(class_reg, &cls.ctor);
        for m in &cls.methods {
            self.bind_method(class_reg, m);
        }
        class_reg
    }

    /// Lower a method/constructor closure and bind it on the class with the
    /// `Method` opcode (legacy `class.rs`).
    fn bind_method(&mut self, class_reg: u8, m: &HirMethod) {
        let mark = self.next_temp;
        let method_reg = self.lower_closure(&m.func, &m.upvalues);
        let key_idx = self.chunk.add_str(&m.key);
        self.chunk.emit(OpCode::Method, self.line);
        self.chunk.write(Chunk::pack(class_reg, method_reg), self.line);
        self.chunk.write(key_idx, self.line);
        self.free_to(mark);
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
            HirBinding::Upvalue(uv) => {
                // LoadUpvalue: dest in hi byte, uv index in lo (legacy `emit_load_var`).
                self.chunk
                    .emit1(OpCode::LoadUpvalue, Chunk::pack(r, *uv as u8), self.line);
            }
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

    /// Self-recursion ABI: no callee load. `LoadNull` receiver, contiguous
    /// args, then `CallSelf dest`. Mirrors legacy `emit_self_call`; the JIT
    /// compiles this to a direct recursive machine call (no VM re-entry).
    fn lower_self_call(&mut self, args: &[HirExpr]) -> u8 {
        let dest = self.alloc();
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
        self.chunk.emit(OpCode::CallSelf, self.line);
        self.chunk.write(Chunk::pack(dest, 0), self.line);
        self.chunk.write(Chunk::pack(total, recv), self.line);
        // Result lands in dest; free the receiver/args temporaries above it.
        self.free_to(dest as u32 + 1);
        dest
    }

    /// Method-call ABI: receiver in `obj`, args contiguous right after it, then
    /// `CallMethod` with an IC slot (in the opcode's dest field) and the method
    /// name as a constant. Mirrors legacy `emit_method_call`.
    fn lower_method_call(&mut self, recv: &HirExpr, name: &str, args: &[HirExpr]) -> u8 {
        let obj = self.lower_expr(recv);
        let arg_start = self.next_temp as u8;
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
        let dest = obj;
        let name_idx = self.chunk.add_str(name);
        let cs = self.alloc_cache() as u8;
        self.chunk
            .write(Chunk::pack_op(OpCode::CallMethod, cs), self.line);
        self.chunk.write(Chunk::pack(dest, obj), self.line);
        self.chunk.write(name_idx, self.line);
        self.chunk
            .write(Chunk::pack(args.len() as u8, arg_start), self.line);
        // Result lands in dest = obj; free the args temporaries above it.
        self.free_to(obj as u32 + 1);
        dest
    }

    /// Short-circuiting `&&`/`||`/`??` → branch + `Move` into `dest`. `dest` is
    /// the first temp allocated so the result lands where the caller expects
    /// (the `lower_expr` net-`+1` contract). Mirrors legacy `compile_logical`.
    fn lower_logical(&mut self, op: HirLogicalOp, lhs: &HirExpr, rhs: &HirExpr) -> u8 {
        let dest = self.alloc();
        let l = self.lower_expr(lhs);
        self.chunk.emit_rr(OpCode::Move, dest, l, self.line);
        self.free_to(dest as u32 + 1);
        match op {
            HirLogicalOp::And => {
                let skip = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, dest, self.line);
                let r = self.lower_expr(rhs);
                self.chunk.emit_rr(OpCode::Move, dest, r, self.line);
                self.free_to(dest as u32 + 1);
                self.chunk.patch_jump(skip);
            }
            HirLogicalOp::Or => {
                let skip = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, dest, self.line);
                let r = self.lower_expr(rhs);
                self.chunk.emit_rr(OpCode::Move, dest, r, self.line);
                self.free_to(dest as u32 + 1);
                self.chunk.patch_jump(skip);
            }
            HirLogicalOp::Nullish => {
                let is_null = self.alloc();
                self.chunk.emit_rr(OpCode::IsNull, is_null, dest, self.line);
                let not_null = self
                    .chunk
                    .emit_cond_jump(OpCode::JumpIfFalse, is_null, self.line);
                self.free_to(dest as u32 + 1);
                let r = self.lower_expr(rhs);
                self.chunk.emit_rr(OpCode::Move, dest, r, self.line);
                self.free_to(dest as u32 + 1);
                self.chunk.patch_jump(not_null);
            }
        }
        dest
    }

    /// Ternary `test ? cons : alt`. The condition register is freed before
    /// `dest` is allocated so `dest` reuses the lowest free slot, keeping the
    /// `lower_expr` net-`+1` contract. Mirrors legacy `compile_conditional`.
    fn lower_conditional(&mut self, test: &HirExpr, cons: &HirExpr, alt: &HirExpr) -> u8 {
        let mark = self.next_temp;
        let cond = self.lower_expr(test);
        let else_j = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
        // The jump has captured `cond`'s register; reclaim it for `dest` so the
        // result lands at `mark` (the branches overwrite it only after the jump).
        self.free_to(mark);
        let dest = self.alloc();
        let c = self.lower_expr(cons);
        self.chunk.emit_rr(OpCode::Move, dest, c, self.line);
        self.free_to(dest as u32 + 1);
        let end_j = self.chunk.emit_jump(OpCode::Jump, self.line);
        self.chunk.patch_jump(else_j);
        let a = self.lower_expr(alt);
        self.chunk.emit_rr(OpCode::Move, dest, a, self.line);
        self.free_to(dest as u32 + 1);
        self.chunk.patch_jump(end_j);
        dest
    }

    /// `++`/`--` on a binding. Postfix yields the old value (already in `cur`);
    /// prefix moves the new value back into `cur`. Mirrors legacy
    /// `compile_update` for the identifier case.
    fn lower_update(&mut self, target: &HirBinding, op: HirUpdateOp, prefix: bool) -> u8 {
        let cur = self.load_binding(target);
        let one = self.alloc();
        self.chunk.emit_load_int(one, 1, self.line);
        let next = self.alloc();
        let opcode = match op {
            HirUpdateOp::Inc => OpCode::Add,
            HirUpdateOp::Dec => OpCode::Sub,
        };
        self.chunk.emit_rrr(opcode, next, cur, one, self.line);
        self.store_binding(target, next);
        if prefix {
            self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
        }
        self.free_to(cur as u32 + 1);
        cur
    }

    /// Array literal (no spread/holes): elements lowered contiguously above
    /// `dest`, then `BuildArray dest, start, count`. Mirrors legacy
    /// `compile_array`'s simple path.
    fn lower_array(&mut self, elements: &[HirExpr]) -> u8 {
        let dest = self.alloc();
        let start = self.next_temp as u8;
        for el in elements {
            let _ = self.lower_expr(el);
        }
        let count = elements.len() as u8;
        self.chunk.emit(OpCode::BuildArray, self.line);
        self.chunk.write(Chunk::pack(dest, start), self.line);
        self.chunk.write(Chunk::pack(count, 0), self.line);
        self.free_to(dest as u32 + 1);
        dest
    }

    /// Fixed-shape object literal: values lowered contiguously above `dest`,
    /// then `BuildObjectWithShape dest, start, shape`. Mirrors legacy
    /// `compile_object`'s `is_simple` path.
    fn lower_object(&mut self, keys: &[Rc<str>], values: &[HirExpr]) -> u8 {
        let shape_idx = self.chunk.add_constant(PoolEntry::Shape(keys.to_vec()));
        let dest = self.alloc();
        let start = self.next_temp as u8;
        for v in values {
            let _ = self.lower_expr(v);
        }
        self.chunk.emit(OpCode::BuildObjectWithShape, self.line);
        self.chunk.write(Chunk::pack(dest, start), self.line);
        self.chunk.write(shape_idx, self.line);
        self.free_to(dest as u32 + 1);
        dest
    }

    /// Closure value: lower the nested function to a proto constant, then emit
    /// `MakeClosure` (or `LoadStaticFn` when it captures nothing). Upvalue
    /// sources are resolved to `(is_local, index)` against this (parent) frame's
    /// register layout. Mirrors legacy `emit_closure`.
    fn lower_closure(&mut self, func: &HirFunction, upvalues: &[HirUpvalueSrc]) -> u8 {
        let proto = lower_function(func, self.chunk.source_file.clone());
        let proto_idx = self
            .chunk
            .add_constant(PoolEntry::Function(Rc::new(proto)));
        let dest = self.alloc();
        if upvalues.is_empty() {
            self.chunk
                .write(Chunk::pack_op(OpCode::LoadStaticFn, dest), self.line);
            self.chunk.write(proto_idx, self.line);
            return dest;
        }
        let uv_count = upvalues.len() as u8;
        self.chunk.emit(OpCode::MakeClosure, self.line);
        self.chunk.write(Chunk::pack(dest, uv_count), self.line);
        self.chunk.write(proto_idx, self.line);
        for uv in upvalues {
            let (is_local, index) = match uv {
                HirUpvalueSrc::ParentLocal(id) => (1u8, self.local_reg(*id)),
                HirUpvalueSrc::ParentParam(i) => (1u8, self.param_reg(*i)),
                HirUpvalueSrc::ParentUpvalue(uv) => (0u8, *uv as u8),
            };
            self.chunk.write(Chunk::pack(is_local, index), self.line);
        }
        dest
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
