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
        for m in &en.methods {
            self.bind_method(class_reg, m);
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
    pub(super) fn lower_class_value(&mut self, cls: &HirClass) -> u8 {
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
}
