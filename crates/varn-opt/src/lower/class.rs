//! Class / enum / `match` value lowering to bytecode.

use varn_core::OpCode;
use varn_types::chunk::Chunk;

use super::FnLower;
use crate::hir::*;

impl FnLower {
    /// Build an enum value: `MakeClass`, then per variant a `MakeEnumVariant`
    /// bound as a `DefineStatic`, then instance fields/methods. Mirrors legacy
    /// `compile_enum_expr`.
    pub(super) fn lower_enum_value(&mut self, en: &HirEnum) -> u8 {
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
        for (key, init) in &en.static_fields {
            let mark = self.next_temp;
            let val_reg = match init {
                Some(e) => self.lower_expr(e),
                None => {
                    let r = self.alloc();
                    self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                    r
                }
            };
            let key_idx = self.chunk.add_str(key);
            self.chunk.emit(OpCode::DefineStatic, self.line);
            self.chunk.write(Chunk::pack(class_reg, val_reg), self.line);
            self.chunk.write(key_idx, self.line);
            self.free_to(mark);
        }
        self.bind_method(class_reg, &en.ctor, false);
        for m in &en.methods {
            self.bind_method(class_reg, m, false);
        }
        for m in &en.static_methods {
            self.bind_method(class_reg, m, true);
        }
        for a in &en.getters {
            let op = if a.is_static {
                OpCode::DefineStaticGetter
            } else {
                OpCode::DefineGetter
            };
            self.bind_member(class_reg, op, &a.key, &a.func, &a.upvalues);
        }
        for a in &en.setters {
            let op = if a.is_static {
                OpCode::DefineStaticSetter
            } else {
                OpCode::DefineSetter
            };
            self.bind_member(class_reg, op, &a.key, &a.func, &a.upvalues);
        }
        let ctor_key = self.chunk.add_str("constructor");
        for v in &en.variants {
            if !v.const_args.is_empty() {
                let mark = self.next_temp;
                let var_name_idx = self.chunk.add_str(&v.name);
                let receiver_reg = self.alloc();
                self.emit_property(OpCode::GetProperty, receiver_reg, class_reg, var_name_idx);

                let mut arg_regs = Vec::new();
                for arg in &v.const_args {
                    arg_regs.push(self.lower_expr(arg));
                }

                let ctor_reg = self.alloc();
                self.emit_property(OpCode::GetProperty, ctor_reg, receiver_reg, ctor_key);

                let dest = self.alloc();
                let arg_count = (1 + arg_regs.len()) as u8;
                self.chunk.emit(OpCode::Call, self.line);
                self.chunk.write(Chunk::pack(dest, ctor_reg), self.line);
                self.chunk.write(Chunk::pack(arg_count, receiver_reg), self.line);

                self.free_to(mark);
            }
        }
        // Pre-bind the class as a global so static block bodies can reference
        // the enum by name (mirrors legacy `__class_X__` local binding at
        // compile_enum_expr:403 which closures capture as an upvalue).
        if !en.static_blocks.is_empty() {
            let pre_bind_idx = self.chunk.add_str(&en.name);
            self.chunk.emit_rrc(OpCode::DefineGlobal, 0, class_reg, pre_bind_idx, self.line);
        }
        for b in &en.static_blocks {
            let mark = self.next_temp;
            let fn_reg = self.lower_closure(&b.func, &b.upvalues);
            let result = self.alloc();
            self.chunk.emit(OpCode::Call, self.line);
            self.chunk.write(Chunk::pack(result, fn_reg), self.line);
            self.chunk.write(Chunk::pack(0, 0), self.line);
            self.free_to(mark);
        }
        class_reg
    }

    /// `match` → a branch chain. `subj` and `dest` sit at the two lowest temps;
    /// each case tests/binds, runs its body, and `Move`s its result into `dest`.
    /// Mirrors legacy `compile_match`.
    pub(super) fn lower_match_value(&mut self, subject: &HirExpr, cases: &[HirMatchCase]) -> u8 {
        let subj = self.lower_expr(subject);
        let dest = self.alloc();
        self.chunk.emit_rr(OpCode::LoadNull, dest, 0, self.line);
        let mut end_jumps: Vec<usize> = Vec::new();
        for case in cases {
            let mark = self.next_temp;
            let skip = self.lower_case_test(&case.test, subj);
            // Guard: pattern matched but guard falsy → fall through to next case.
            let guard_skip = match &case.guard {
                Some(g) => {
                    let gmark = self.next_temp;
                    let gr = self.lower_expr(g);
                    let j = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, gr, self.line);
                    self.free_to(gmark);
                    Some(j)
                }
                None => None,
            };
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
            if let Some(gs) = guard_skip {
                self.chunk.patch_jump(gs);
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
            HirCaseTest::Record { fields } => {
                for (field_name, option_local) in fields {
                    if let Some(local) = option_local {
                        let fkey = self.chunk.add_str(field_name);
                        let field_r = self.alloc();
                        self.emit_property(OpCode::GetProperty, field_r, subj, fkey);
                        let dst = self.local_reg(*local);
                        self.chunk.emit_rr(OpCode::Move, dst, field_r, self.line);
                        self.free();
                    }
                }
                None
            }
        }
    }

    /// Build a class value: `MakeClass`, then fields, static fields, the
    /// constructor + instance methods, static methods, getters/setters, and
    /// immediately-invoked static blocks. Mirrors legacy `compile_class_expr`.
    pub(super) fn lower_class_value(&mut self, cls: &HirClass) -> u8 {
        let name_idx = self.chunk.add_str(&cls.name);
        // A superclass value (`extends`) is passed in `MakeClass`'s second
        // operand so the VM links the prototype chain; 0 means no base. Allocate
        // `class_reg` first so the superclass temp sits above it and is freed.
        let class_reg = self.alloc();
        let super_reg = match &cls.super_class {
            Some(sup) => self.lower_expr(sup),
            None => 0,
        };
        self.chunk
            .emit_rrc(OpCode::MakeClass, class_reg, super_reg, name_idx, self.line);
        if cls.super_class.is_some() {
            self.free_to(class_reg as u32 + 1);
        }
        for field in &cls.fields {
            let key_idx = self.chunk.add_str(field);
            self.chunk.emit(OpCode::DeclareField, self.line);
            self.chunk.write(Chunk::pack(class_reg, 0), self.line);
            self.chunk.write(key_idx, self.line);
        }
        for (key, init) in &cls.static_fields {
            let mark = self.next_temp;
            let val_reg = match init {
                Some(e) => self.lower_expr(e),
                None => {
                    let r = self.alloc();
                    self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                    r
                }
            };
            let key_idx = self.chunk.add_str(key);
            self.chunk.emit(OpCode::DefineStatic, self.line);
            self.chunk.write(Chunk::pack(class_reg, val_reg), self.line);
            self.chunk.write(key_idx, self.line);
            self.free_to(mark);
        }
        self.bind_method(class_reg, &cls.ctor, false);
        for m in &cls.methods {
            self.bind_method(class_reg, m, false);
        }
        for m in &cls.static_methods {
            self.bind_method(class_reg, m, true);
        }
        for a in &cls.getters {
            let op = if a.is_static {
                OpCode::DefineStaticGetter
            } else {
                OpCode::DefineGetter
            };
            self.bind_member(class_reg, op, &a.key, &a.func, &a.upvalues);
        }
        for a in &cls.setters {
            let op = if a.is_static {
                OpCode::DefineStaticSetter
            } else {
                OpCode::DefineSetter
            };
            self.bind_member(class_reg, op, &a.key, &a.func, &a.upvalues);
        }
        for b in &cls.static_blocks {
            let mark = self.next_temp;
            let fn_reg = self.lower_closure(&b.func, &b.upvalues);
            let result = self.alloc();
            self.chunk.emit(OpCode::Call, self.line);
            self.chunk.write(Chunk::pack(result, fn_reg), self.line);
            self.chunk.write(Chunk::pack(0, 0), self.line);
            self.free_to(mark);
        }
        self.apply_class_decorators(class_reg, &cls.decorators);
        class_reg
    }

    fn bind_method(&mut self, class_reg: u8, m: &HirMethod, is_static: bool) {
        let op = if is_static { OpCode::DefineStatic } else { OpCode::Method };
        let mark = self.next_temp;
        let mut reg = self.lower_closure(&m.func, &m.upvalues);

        if !m.decorators.is_empty() {
            reg = self.apply_method_decorators(reg, &m.key, is_static, m.is_private, &m.decorators);
        }

        let key_idx = self.chunk.add_str(&m.key);
        self.chunk.emit(op, self.line);
        self.chunk.write(Chunk::pack(class_reg, reg), self.line);
        self.chunk.write(key_idx, self.line);
        self.free_to(mark);
    }

    fn apply_method_decorators(
        &mut self,
        method_reg: u8,
        key: &str,
        is_static: bool,
        is_private: bool,
        decorators: &[HirExpr],
    ) -> u8 {
        for deco in decorators.iter().rev() {
            let mark = self.next_temp;
            let deco_fn = self.lower_expr(deco);

            let k_name = self.chunk.add_str("name");
            let v_name = self.chunk.add_str(key);
            let k_kind = self.chunk.add_str("kind");
            let v_kind = self.chunk.add_str("method");
            let k_static = self.chunk.add_str("isStatic");
            let k_private = self.chunk.add_str("isPrivate");

            let n_r = self.alloc();
            self.chunk.emit_rc(OpCode::LoadConst, n_r, v_name, self.line);
            let kind_r = self.alloc();
            self.chunk.emit_rc(OpCode::LoadConst, kind_r, v_kind, self.line);
            let static_r = self.alloc();
            self.chunk.emit_rr(
                if is_static { OpCode::LoadTrue } else { OpCode::LoadFalse },
                static_r,
                0,
                self.line,
            );
            let private_r = self.alloc();
            self.chunk.emit_rr(
                if is_private { OpCode::LoadTrue } else { OpCode::LoadFalse },
                private_r,
                0,
                self.line,
            );

            let ctx_reg = self.alloc();
            self.chunk.emit(OpCode::BuildObject, self.line);
            self.chunk.write(Chunk::pack(ctx_reg, 4), self.line);
            self.chunk.write(k_name, self.line);
            self.chunk.write(Chunk::pack(n_r, 0), self.line);
            self.chunk.write(k_kind, self.line);
            self.chunk.write(Chunk::pack(kind_r, 0), self.line);
            self.chunk.write(k_static, self.line);
            self.chunk.write(Chunk::pack(static_r, 0), self.line);
            self.chunk.write(k_private, self.line);
            self.chunk.write(Chunk::pack(private_r, 0), self.line);

            let recv_reg = self.alloc();
            self.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, self.line);

            let a0 = self.alloc();
            self.chunk.emit_rr(OpCode::Move, a0, method_reg, self.line);

            let a1 = self.alloc();
            self.chunk.emit_rr(OpCode::Move, a1, ctx_reg, self.line);

            let result = self.alloc();

            self.chunk.emit(OpCode::Call, self.line);
            self.chunk.write(Chunk::pack(result, deco_fn), self.line);
            self.chunk.write(Chunk::pack(3, recv_reg), self.line);

            let is_null = self.alloc();
            self.chunk.emit_rr(OpCode::IsNull, is_null, result, self.line);
            let skip = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, is_null, self.line);
            self.chunk.emit_rr(OpCode::Move, method_reg, result, self.line);
            self.chunk.patch_jump(skip);

            self.free_to(mark);
        }
        method_reg
    }

    fn apply_class_decorators(&mut self, class_reg: u8, decorators: &[HirExpr]) {
        for deco in decorators.iter().rev() {
            let mark = self.next_temp;
            let deco_reg = self.lower_expr(deco);
            let result = self.alloc();
            let is_null = self.alloc();

            let recv_reg = self.alloc();
            self.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, self.line);

            let arg_class_reg = self.alloc();
            self.chunk.emit_rr(OpCode::Move, arg_class_reg, class_reg, self.line);

            self.chunk.emit(OpCode::Call, self.line);
            self.chunk.write(Chunk::pack(result, deco_reg), self.line);
            self.chunk.write(Chunk::pack(2, recv_reg), self.line);

            self.chunk.emit_rr(OpCode::IsNull, is_null, result, self.line);
            let skip = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, is_null, self.line);
            self.chunk.emit_rr(OpCode::Move, class_reg, result, self.line);
            self.chunk.patch_jump(skip);

            self.free_to(mark);
        }
    }

    /// Lower a closure and bind it on the class with `op` (`Method`/
    /// `DefineStatic`/`DefineGetter`/`DefineSetter`/static accessor variants),
    /// which all share the `pack(class, reg) + key` encoding.
    fn bind_member(
        &mut self,
        class_reg: u8,
        op: OpCode,
        key: &str,
        func: &HirFunction,
        upvalues: &[HirUpvalueSrc],
    ) {
        let mark = self.next_temp;
        let reg = self.lower_closure(func, upvalues);
        let key_idx = self.chunk.add_str(key);
        self.chunk.emit(op, self.line);
        self.chunk.write(Chunk::pack(class_reg, reg), self.line);
        self.chunk.write(key_idx, self.line);
        self.free_to(mark);
    }
}
