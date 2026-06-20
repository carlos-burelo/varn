//! Expression lowering: `HirExpr -> bytecode`, returning the register holding
//! the result. The register model mirrors the legacy codegen (see the crate
//! module docs): every read materialises into a fresh temporary, so each
//! `lower_expr` call has a net `+1` register effect.

use std::rc::Rc;

use varn_core::OpCode;
use varn_types::chunk::{Chunk, Literal, PoolEntry};

use super::{bin_opcode, lower_function, FnLower};
use crate::hir::*;

impl FnLower {
    /// Lower an expression, returning the register holding its value.
    pub(super) fn lower_expr(&mut self, expr: &HirExpr) -> u8 {
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
            HirExpr::Char(c) => {
                let idx = self
                    .chunk
                    .add_constant(PoolEntry::Literal(Literal::Char(*c)));
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::Decimal(d) => {
                let idx = self
                    .chunk
                    .add_constant(PoolEntry::Literal(Literal::Decimal(*d)));
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::BigInt(bi) => {
                let idx = self
                    .chunk
                    .add_constant(PoolEntry::Literal(Literal::BigInt(*bi)));
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::Regex { pattern, flags } => {
                let raw = format!("/{pattern}/{flags}");
                let idx = self.chunk.add_str(&raw);
                let r = self.alloc();
                self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                r
            }
            HirExpr::Null => {
                let r = self.alloc();
                self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                r
            }
            HirExpr::NonNull(operand) => {
                let r = self.lower_expr(operand);
                // The VM reads the operand from the *high* byte (`hi(w1)`), and
                // `regalloc_post` remaps that high byte — so the value reg must
                // be packed high (legacy relies on its IR-regalloc to fix up its
                // `pack(0, r)`; we have no IR, so we emit the final layout).
                self.chunk
                    .emit1(OpCode::AssertNotNull, Chunk::pack(r, 0), self.line);
                r
            }
            HirExpr::TryOp(operand) => {
                // `expr?`: early-return the value if it is a non-ok enum, else
                // fall through with it (legacy `compile_try_expr`). The operand
                // reg is a fresh temp (`load_binding` copies), so it survives as
                // the result.
                let r = self.lower_expr(operand);
                let tag = self.alloc();
                self.chunk.emit_rr(OpCode::GetEnumTag, tag, r, self.line);
                let ok = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, tag, self.line);
                self.free_to(tag as u32); // tag consumed by the branch; keep r
                self.chunk
                    .emit1(OpCode::Return, Chunk::pack(0, r), self.line);
                self.chunk.patch_jump(ok);
                r
            }
            HirExpr::TypeTest { value, kind } => {
                // `expr is Type` → a concrete check, result reused into the
                // operand's (fresh-temp) reg. Mirrors legacy `compile_is`.
                let src = self.lower_expr(value);
                match kind {
                    HirTypeTest::IsNull => self.chunk.emit_rr(OpCode::IsNull, src, src, self.line),
                    HirTypeTest::IsArray => self.chunk.emit_rr(OpCode::IsArray, src, src, self.line),
                    HirTypeTest::TypeofEq(name) => {
                        let type_str = self.alloc();
                        self.chunk.emit_rr(OpCode::Typeof, type_str, src, self.line);
                        let s_idx = self.chunk.add_str(name);
                        let s_reg = self.alloc();
                        self.chunk.emit_rc(OpCode::LoadConst, s_reg, s_idx, self.line);
                        self.chunk.emit_rrr(OpCode::Eq, src, type_str, s_reg, self.line);
                        self.free_to(src as u32 + 1); // free type_str + s_reg
                    }
                    HirTypeTest::Instanceof(name) => {
                        let cls = self.alloc();
                        let idx = self.chunk.add_str(name);
                        self.chunk.emit_rc(OpCode::LoadGlobal, cls, idx, self.line);
                        self.chunk
                            .emit_rrr(OpCode::Instanceof, src, src, cls, self.line);
                        self.free_to(src as u32 + 1); // free cls
                    }
                    HirTypeTest::AlwaysFalse => {
                        self.chunk.emit_rr(OpCode::LoadFalse, src, 0, self.line)
                    }
                }
                src
            }
            HirExpr::Sequence(exprs) => {
                let mark = self.next_temp;
                let mut last = None;
                for (i, e) in exprs.iter().enumerate() {
                    if i + 1 < exprs.len() {
                        let _ = self.lower_expr(e);
                        self.free_to(mark); // discard intermediate results
                    } else {
                        last = Some(self.lower_expr(e));
                    }
                }
                last.unwrap_or_else(|| {
                    let r = self.alloc();
                    self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                    r
                })
            }
            HirExpr::Range {
                start,
                end,
                inclusive,
            } => self.lower_range(start, end, *inclusive),
            HirExpr::Template(parts) => self.lower_template(parts),
            HirExpr::Assign { target, value } => self.lower_assign_expr(target, value),
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
                match op {
                    HirUnOp::Neg => self.chunk.emit_rr(OpCode::Negate, s, s, self.line),
                    HirUnOp::Not => self.chunk.emit_rr(OpCode::Not, s, s, self.line),
                    HirUnOp::Typeof => self.chunk.emit_rr(OpCode::Typeof, s, s, self.line),
                    HirUnOp::BitNot => {
                        // `~x` == `x ^ -1` (legacy `compile_unary`).
                        let idx = self
                            .chunk
                            .add_constant(PoolEntry::Literal(Literal::Int(-1)));
                        let neg1 = self.alloc();
                        self.chunk.emit_rc(OpCode::LoadConst, neg1, idx, self.line);
                        self.chunk.emit_rrr(OpCode::BitXor, s, s, neg1, self.line);
                        self.free(); // free neg1; result stays in s
                    }
                }
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
            HirExpr::Object { properties } => self.lower_object(properties),
            HirExpr::Closure { func, upvalues } => self.lower_closure(func, upvalues),
            HirExpr::This => {
                // The receiver lives in register 0; copy it into a temp.
                let r = self.alloc();
                self.chunk.emit_rr(OpCode::Move, r, 0, self.line);
                r
            }
            HirExpr::ExtensionCall { func, recv, args } => self.lower_ext_call(func, recv, args),
            HirExpr::SuperCall { args } => self.lower_super_call(args),
            HirExpr::SuperMethodCall { name, args } => self.lower_super_method_call(name, args),
            HirExpr::SuperMember { name } => {
                let r = self.alloc();
                let idx = self.chunk.add_str(name);
                self.chunk.emit_rc(OpCode::GetSuper, r, idx, self.line);
                r
            }
            HirExpr::Super => {
                let r = self.alloc();
                let idx = self.chunk.add_str("super");
                self.chunk.emit_rc(OpCode::GetSuper, r, idx, self.line);
                r
            }
            HirExpr::ModuleSlot { object, slot, ty: _ } => {
                let obj = self.lower_expr(object);
                let dest = self.alloc();
                self.chunk.emit_rrc(OpCode::LoadModuleSlot, dest, obj, *slot, self.line);
                self.free(); // free obj
                dest
            }
            HirExpr::Class(cls) => self.lower_class_value(cls),
            HirExpr::Enum(en) => self.lower_enum_value(en),
            HirExpr::Match { subject, cases } => self.lower_match_value(subject, cases),
            HirExpr::MemberMaybe { object, name, .. } => {
                let obj = self.lower_expr(object);
                let name_idx = self.chunk.add_str(name);
                self.chunk.emit_rrc(OpCode::GetPropertyMaybe, obj, obj, name_idx, self.line);
                obj
            }
            HirExpr::ObjectRest { object, skip_keys } => {
                let obj = self.lower_expr(object);
                let dest = self.alloc();
                let line = self.line;
                self.chunk.emit(OpCode::ObjectRest, line);
                self.chunk.write(Chunk::pack(dest, obj), line);
                self.chunk.write(Chunk::pack(skip_keys.len() as u8, 0), line);
                for k in skip_keys {
                    let key_idx = self.chunk.add_str(k);
                    self.chunk.write(key_idx, line);
                }
                self.chunk.emit_rr(OpCode::Move, obj, dest, line);
                self.free();
                obj
            }
            HirExpr::OptionalChain { object, property } => {
                let obj = self.lower_expr(object);
                let is_null = self.alloc();
                self.chunk.emit_rr(OpCode::IsNull, is_null, obj, self.line);
                let end = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, is_null, self.line);
                self.free();

                match property {
                    HirOptionalProperty::Member(name) => {
                        let name_idx = self.chunk.add_str(name);
                        self.chunk.emit_rrc(OpCode::GetPropertyMaybe, obj, obj, name_idx, self.line);
                    }
                    HirOptionalProperty::Index(index) => {
                        let key = self.lower_expr(index);
                        self.chunk.emit_rrr(OpCode::GetIndex, obj, obj, key, self.line);
                        self.free();
                    }
                    HirOptionalProperty::ModuleSlot(slot_idx) => {
                        self.chunk.emit_rrc(OpCode::LoadModuleSlot, obj, obj, *slot_idx, self.line);
                    }
                    HirOptionalProperty::Extension(mangled) => {
                        let ext_fn = self.alloc();
                        let idx = self.chunk.add_str(mangled);
                        self.chunk.emit_rc(OpCode::LoadGlobal, ext_fn, idx, self.line);
                        let dest = self.alloc();
                        let line = self.line;
                        self.chunk.emit(OpCode::Call, line);
                        self.chunk.write(Chunk::pack(dest, ext_fn), line);
                        self.chunk.write(Chunk::pack(1, obj), line);
                        self.chunk.emit_rr(OpCode::Move, obj, dest, line);
                        self.free();
                        self.free();
                    }
                    HirOptionalProperty::Call(args) => {
                        let recv = self.alloc();
                        self.chunk.emit_rr(OpCode::LoadNull, recv, 0, self.line);
                        let arg_start = recv + 1;
                        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
                        for (i, a) in args.iter().enumerate() {
                            let slot = arg_start + i as u8;
                            match a {
                                HirExpr::Spread(inner) => {
                                    let r = self.lower_expr(inner);
                                    if r != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                                    } else {
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                                    }
                                }
                                _ => {
                                    let r = self.lower_expr(a);
                                    if r != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                                    }
                                }
                            }
                        }
                        let total = (args.len() + 1) as u8;
                        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
                        self.chunk.emit(op, self.line);
                        self.chunk.write(Chunk::pack(obj, obj), self.line);
                        self.chunk.write(Chunk::pack(total, recv), self.line);
                        self.free_to(obj as u32 + 1);
                    }
                    HirOptionalProperty::MethodCall(name, args) => {
                        let arg_start = self.next_temp as u8;
                        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
                        for (i, a) in args.iter().enumerate() {
                            let slot = arg_start + i as u8;
                            match a {
                                HirExpr::Spread(inner) => {
                                    let r = self.lower_expr(inner);
                                    if r != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                                    } else {
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                                    }
                                }
                                _ => {
                                    let r = self.lower_expr(a);
                                    if r != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                                    }
                                }
                            }
                        }
                        let dest = obj;
                        let name_idx = self.chunk.add_str(name);
                        if has_spread {
                            let fn_reg = self.alloc();
                            self.emit_property(OpCode::GetProperty, fn_reg, obj, name_idx);
                            self.chunk.emit(OpCode::CallSpread, self.line);
                            self.chunk.write(Chunk::pack(dest, fn_reg), self.line);
                            self.chunk.write(Chunk::pack(args.len() as u8, arg_start), self.line);
                            self.free();
                        } else {
                            let cs = self.alloc_cache() as u8;
                            self.chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), self.line);
                            self.chunk.write(Chunk::pack(dest, obj), self.line);
                            self.chunk.write(name_idx, self.line);
                            self.chunk.write(Chunk::pack(args.len() as u8, arg_start), self.line);
                        }
                        self.free_to(obj as u32 + 1);
                    }
                    HirOptionalProperty::ExtensionCall(mangled, args) => {
                        let fn_reg = self.alloc();
                        let idx = self.chunk.add_str(mangled);
                        self.chunk.emit_rc(OpCode::LoadGlobal, fn_reg, idx, self.line);
                        let recv_slot = self.alloc();
                        self.chunk.emit_rr(OpCode::Move, recv_slot, obj, self.line);
                        let arg_start = recv_slot + 1;
                        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
                        for (i, a) in args.iter().enumerate() {
                            let slot = arg_start + i as u8;
                            match a {
                                HirExpr::Spread(inner) => {
                                    let rr = self.lower_expr(inner);
                                    if rr != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, rr, self.line);
                                    } else {
                                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                                    }
                                }
                                _ => {
                                    let rr = self.lower_expr(a);
                                    if rr != slot {
                                        while (self.next_temp as u8) <= slot {
                                            self.alloc();
                                        }
                                        self.chunk.emit_rr(OpCode::Move, slot, rr, self.line);
                                    }
                                }
                            }
                        }
                        let total = (args.len() + 1) as u8;
                        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
                        self.chunk.emit(op, self.line);
                        self.chunk.write(Chunk::pack(fn_reg, fn_reg), self.line);
                        self.chunk.write(Chunk::pack(total, recv_slot), self.line);
                        self.chunk.emit_rr(OpCode::Move, obj, fn_reg, self.line);
                        self.free_to(obj as u32 + 1);
                    }
                }

                self.chunk.patch_jump(end);
                obj
            }
            HirExpr::Await(inner) => {
                let src = self.lower_expr(inner);
                self.chunk.emit_rr(OpCode::Await, src, src, self.line);
                src
            }
            HirExpr::Spawn(inner) => {
                let src = self.lower_expr(inner);
                self.chunk.emit2(
                    OpCode::Spawn,
                    Chunk::pack(src, src),
                    0,
                    self.line,
                );
                src
            }
            HirExpr::Yield(inner) => {
                let src = self.lower_expr(inner);
                self.chunk.emit1(
                    OpCode::Yield,
                    Chunk::pack(src, src),
                    self.line,
                );
                src
            }
            HirExpr::TaggedTemplate { tag, template } => {
                let tag_reg = self.lower_expr(tag);
                let tpl_reg = self.lower_expr(template);
                let dest = self.alloc();
                let line = self.line;
                self.chunk.emit(OpCode::Call, line);
                self.chunk.write(Chunk::pack(dest, tag_reg), line);
                self.chunk.write(Chunk::pack(1, tpl_reg), line);
                self.free_to(dest as u32 + 1);
                dest
            }
            HirExpr::IntrinsicCall { object, args, wire_byte, .. } => {
                let dest = self.lower_expr(object);
                let mut arg_count = 1usize;
                for arg in args {
                    let expected = dest + arg_count as u8;
                    let r = self.lower_expr(arg);
                    if r != expected {
                        while (self.next_temp as u8) <= expected {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, expected, r, self.line);
                    }
                    arg_count += 1;
                }
                let line = self.line;
                self.chunk.write(Chunk::pack_op(OpCode::Intrinsic, dest), line);
                self.chunk.write(((*wire_byte as u16) << 8) | (arg_count as u16), line);
                self.free_to(dest as u32 + 1);
                dest
            }
            HirExpr::Spread(_) => {
                unreachable!("Spread argument should only be evaluated inside array/call lowers");
            }
        }
    }

    pub(super) fn load_binding(&mut self, binding: &HirBinding) -> u8 {
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
        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            match a {
                HirExpr::Spread(inner) => {
                    let r = self.lower_expr(inner);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                    } else {
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                    }
                }
                _ => {
                    let r = self.lower_expr(a);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                    }
                }
            }
        }
        let total = (args.len() + 1) as u8; // receiver + args
        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
        self.chunk.emit(op, self.line);
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

    /// Extension call ABI: `LoadGlobal __ext*`, then the receiver in the call's
    /// receiver slot (callee reg 0 = `this`) with args contiguous after it.
    /// Mirrors legacy `emit_extension_call`.
    fn lower_ext_call(&mut self, func: &str, recv: &HirExpr, args: &[HirExpr]) -> u8 {
        let fn_reg = self.alloc();
        let idx = self.chunk.add_str(func);
        self.chunk.emit_rc(OpCode::LoadGlobal, fn_reg, idx, self.line);
        let recv_slot = self.alloc();
        let r = self.lower_expr(recv);
        if r != recv_slot {
            while (self.next_temp as u8) <= recv_slot {
                self.alloc();
            }
            self.chunk.emit_rr(OpCode::Move, recv_slot, r, self.line);
        }
        let arg_start = recv_slot + 1;
        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            match a {
                HirExpr::Spread(inner) => {
                    let rr = self.lower_expr(inner);
                    if rr != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, rr, self.line);
                    } else {
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                    }
                }
                _ => {
                    let rr = self.lower_expr(a);
                    if rr != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, slot, rr, self.line);
                    }
                }
            }
        }
        let total = (args.len() + 1) as u8; // receiver + args
        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
        self.chunk.emit(op, self.line);
        self.chunk.write(Chunk::pack(fn_reg, fn_reg), self.line);
        self.chunk.write(Chunk::pack(total, recv_slot), self.line);
        self.free_to(fn_reg as u32 + 1);
        fn_reg
    }

    /// `super(args)` ABI: `GetSuper "constructor"`, receiver = `this` (reg 0)
    /// moved into a temp, contiguous args, then `Call` with receiver+args.
    /// Mirrors legacy `compile_call` super-constructor path.
    fn lower_super_call(&mut self, args: &[HirExpr]) -> u8 {
        let fn_reg = self.alloc();
        let ctor_idx = self.chunk.add_str("constructor");
        self.chunk.emit_rc(OpCode::GetSuper, fn_reg, ctor_idx, self.line);
        let recv = self.alloc();
        self.chunk.emit_rr(OpCode::Move, recv, 0, self.line); // `this`
        let arg_start = recv + 1;
        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            match a {
                HirExpr::Spread(inner) => {
                    let r = self.lower_expr(inner);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                    } else {
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                    }
                }
                _ => {
                    let r = self.lower_expr(a);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                    }
                }
            }
        }
        let total = (args.len() + 1) as u8; // receiver + args
        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
        self.chunk.emit(op, self.line);
        self.chunk.write(Chunk::pack(fn_reg, fn_reg), self.line);
        self.chunk.write(Chunk::pack(total, recv), self.line);
        self.free_to(fn_reg as u32 + 1);
        fn_reg
    }

    /// `super.name(args)` ABI: `GetSuper name` returns a method already bound to
    /// `this`, then a `Call` over the contiguous args (no separate receiver).
    /// Mirrors legacy `compile_call` super-method path.
    fn lower_super_method_call(&mut self, name: &str, args: &[HirExpr]) -> u8 {
        let fn_reg = self.alloc();
        let name_idx = self.chunk.add_str(name);
        self.chunk.emit_rc(OpCode::GetSuper, fn_reg, name_idx, self.line);
        let arg_start = self.next_temp as u8;
        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            match a {
                HirExpr::Spread(inner) => {
                    let r = self.lower_expr(inner);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                    } else {
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                    }
                }
                _ => {
                    let r = self.lower_expr(a);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                    }
                }
            }
        }
        let count = args.len() as u8;
        let op = if has_spread { OpCode::CallSpread } else { OpCode::Call };
        self.chunk.emit(op, self.line);
        self.chunk.write(Chunk::pack(fn_reg, fn_reg), self.line);
        self.chunk.write(
            Chunk::pack(count, if count > 0 { arg_start } else { 0 }),
            self.line,
        );
        self.free_to(fn_reg as u32 + 1);
        fn_reg
    }

    /// Method-call ABI: receiver in `obj`, args contiguous right after it, then
    /// `CallMethod` with an IC slot (in the opcode's dest field) and the method
    /// name as a constant. Mirrors legacy `emit_method_call`.
    fn lower_method_call(&mut self, recv: &HirExpr, name: &str, args: &[HirExpr]) -> u8 {
        let obj = self.lower_expr(recv);
        let arg_start = self.next_temp as u8;
        let has_spread = args.iter().any(|a| matches!(a, HirExpr::Spread(_)));
        for (i, a) in args.iter().enumerate() {
            let slot = arg_start + i as u8;
            match a {
                HirExpr::Spread(inner) => {
                    let r = self.lower_expr(inner);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, r, self.line);
                    } else {
                        self.chunk.emit_rr(OpCode::WrapSpread, slot, slot, self.line);
                    }
                }
                _ => {
                    let r = self.lower_expr(a);
                    if r != slot {
                        while (self.next_temp as u8) <= slot {
                            self.alloc();
                        }
                        self.chunk.emit_rr(OpCode::Move, slot, r, self.line);
                    }
                }
            }
        }
        let dest = obj;
        let name_idx = self.chunk.add_str(name);
        if has_spread {
            let fn_reg = self.alloc();
            self.emit_property(OpCode::GetProperty, fn_reg, obj, name_idx);
            self.chunk.emit(OpCode::CallSpread, self.line);
            self.chunk.write(Chunk::pack(dest, fn_reg), self.line);
            self.chunk.write(Chunk::pack(args.len() as u8, arg_start), self.line);
            self.free();
        } else {
            let cs = self.alloc_cache() as u8;
            self.chunk
                .write(Chunk::pack_op(OpCode::CallMethod, cs), self.line);
            self.chunk.write(Chunk::pack(dest, obj), self.line);
            self.chunk.write(name_idx, self.line);
            self.chunk
                .write(Chunk::pack(args.len() as u8, arg_start), self.line);
        }
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

    /// `++`/`--` on a generalized assignment target.
    fn lower_update(&mut self, target: &HirAssignTarget, op: HirUpdateOp, prefix: bool) -> u8 {
        let opcode = match op {
            HirUpdateOp::Inc => OpCode::Add,
            HirUpdateOp::Dec => OpCode::Sub,
        };

        match target {
            HirAssignTarget::Var(binding) => {
                let cur = self.load_binding(binding);
                let one = self.alloc();
                self.chunk.emit_load_int(one, 1, self.line);
                let next = self.alloc();
                self.chunk.emit_rrr(opcode, next, cur, one, self.line);
                self.store_binding(binding, next);
                if prefix {
                    self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
                }
                self.free_to(cur as u32 + 1);
                cur
            }
            HirAssignTarget::Member { object, name } => {
                let obj = self.lower_expr(object);
                let cur = self.alloc();
                let name_idx = self.chunk.add_str(name);
                self.emit_property(OpCode::GetProperty, cur, obj, name_idx);

                let one = self.alloc();
                self.chunk.emit_load_int(one, 1, self.line);
                let next = self.alloc();
                self.chunk.emit_rrr(opcode, next, cur, one, self.line);
                self.free(); // free one

                self.emit_property(OpCode::SetProperty, obj, next, name_idx);
                self.free(); // free obj

                if prefix {
                    self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
                }
                self.free_to(cur as u32 + 1); // free next (keeps cur)
                cur
            }
            HirAssignTarget::Index { object, index } => {
                let obj = self.lower_expr(object);
                let idx = self.lower_expr(index);
                let cur = self.alloc();
                self.chunk.emit_rrr(OpCode::GetIndex, cur, obj, idx, self.line);

                let one = self.alloc();
                self.chunk.emit_load_int(one, 1, self.line);
                let next = self.alloc();
                self.chunk.emit_rrr(opcode, next, cur, one, self.line);
                self.free(); // free one

                self.chunk.emit_rrr(OpCode::SetIndex, obj, idx, next, self.line);
                self.free(); // free idx
                self.free(); // free obj

                if prefix {
                    self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
                }
                self.free_to(cur as u32 + 1); // free next (keeps cur)
                cur
            }
            HirAssignTarget::SuperMember { name } => {
                let ctor_idx = self.chunk.add_str("constructor");
                let obj = self.alloc();
                self.chunk.emit_rc(OpCode::GetSuper, obj, ctor_idx, self.line);

                let cur = self.alloc();
                let name_idx = self.chunk.add_str(name);
                self.emit_property(OpCode::GetProperty, cur, obj, name_idx);

                let one = self.alloc();
                self.chunk.emit_load_int(one, 1, self.line);
                let next = self.alloc();
                self.chunk.emit_rrr(opcode, next, cur, one, self.line);
                self.free(); // free one

                self.emit_property(OpCode::SetProperty, obj, next, name_idx);
                self.free(); // free obj

                if prefix {
                    self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
                }
                self.free_to(cur as u32 + 1); // free next (keeps cur)
                cur
            }
            HirAssignTarget::SuperIndex { index } => {
                let ctor_idx = self.chunk.add_str("constructor");
                let obj = self.alloc();
                self.chunk.emit_rc(OpCode::GetSuper, obj, ctor_idx, self.line);

                let idx = self.lower_expr(index);
                let cur = self.alloc();
                self.chunk.emit_rrr(OpCode::GetIndex, cur, obj, idx, self.line);

                let one = self.alloc();
                self.chunk.emit_load_int(one, 1, self.line);
                let next = self.alloc();
                self.chunk.emit_rrr(opcode, next, cur, one, self.line);
                self.free(); // free one

                self.chunk.emit_rrr(OpCode::SetIndex, obj, idx, next, self.line);
                self.free(); // free idx
                self.free(); // free obj

                if prefix {
                    self.chunk.emit_rr(OpCode::Move, cur, next, self.line);
                }
                self.free_to(cur as u32 + 1); // free next (keeps cur)
                cur
            }
            HirAssignTarget::ModuleSlot { .. } => {
                unreachable!("ModuleSlot assignment targets are not constructed in frontend lowering")
            }
        }
    }

    /// Array literal (no spread/holes): elements lowered contiguously above
    /// `dest`, then `BuildArray dest, start, count`. Mirrors legacy
    /// `compile_array`'s simple path.
    fn lower_array(&mut self, elements: &[HirArrayEl]) -> u8 {
        let has_spread = elements.iter().any(|e| matches!(e, HirArrayEl::Spread(_)));

        if !has_spread {
            let dest = self.alloc();
            let start = self.next_temp as u8;
            for el in elements {
                match el {
                    HirArrayEl::Hole => {
                        let r = self.alloc();
                        self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                    }
                    HirArrayEl::Expr(e) => {
                        let _ = self.lower_expr(e);
                    }
                    HirArrayEl::Spread(_) => unreachable!(),
                }
            }
            let count = elements.len() as u8;
            self.chunk.emit(OpCode::BuildArray, self.line);
            self.chunk.write(Chunk::pack(dest, start), self.line);
            self.chunk.write(Chunk::pack(count, 0), self.line);
            self.free_to(dest as u32 + 1);
            return dest;
        }

        let arr = self.alloc();
        self.chunk.emit(OpCode::BuildArray, self.line);
        self.chunk.write(Chunk::pack(arr, 0), self.line);
        self.chunk.write(Chunk::pack(0, 0), self.line);

        for el in elements {
            match el {
                HirArrayEl::Hole => {
                    let null_r = self.alloc();
                    self.chunk.emit_rr(OpCode::LoadNull, null_r, 0, self.line);
                    self.chunk.emit_rr(OpCode::ArrayPush, arr, null_r, self.line);
                    self.free();
                }
                HirArrayEl::Expr(e) => {
                    let r = self.lower_expr(e);
                    self.chunk.emit_rr(OpCode::ArrayPush, arr, r, self.line);
                    self.free();
                }
                HirArrayEl::Spread(e) => {
                    let r = self.lower_expr(e);
                    self.chunk.emit_rr(OpCode::ArrayExtend, arr, r, self.line);
                    self.free();
                }
            }
        }
        arr
    }

    /// `start..end` / `start..=end` → `InvokeRuntimeStatic __range__(start, end,
    /// inclusive)`. Mirrors legacy `expr/mod.rs` Range. `dest` is allocated first
    /// so the two args sit contiguously above it (the call's `arg_start`).
    fn lower_range(&mut self, start: &HirExpr, end: &HirExpr, inclusive: bool) -> u8 {
        let dest = self.alloc();
        let s = self.lower_expr(start);
        let e = self.lower_expr(end);
        let method = self.chunk.add_str(varn_core::well_known::RUNTIME_RANGE);
        let flag = if inclusive { 1u8 } else { 0u8 };
        self.chunk.emit(OpCode::InvokeRuntimeStatic, self.line);
        self.chunk.write(Chunk::pack(dest, 0), self.line);
        self.chunk.write(method, self.line);
        self.chunk.write(Chunk::pack(2, s), self.line);
        self.chunk.write(Chunk::pack(e, flag), self.line);
        self.free_to(dest as u32 + 1);
        dest
    }

    /// Template literal → `BuildStr` over each part (interpolations stringified
    /// with `ToString`). `dest` is allocated first so it stays distinct from the
    /// part registers. Mirrors legacy `templates.rs::compile_template`.
    fn lower_template(&mut self, parts: &[HirTemplatePart]) -> u8 {
        let dest = self.alloc();
        if parts.is_empty() {
            let idx = self.chunk.add_str("");
            self.chunk.emit_rc(OpCode::LoadConst, dest, idx, self.line);
            return dest;
        }
        let mut part_regs: Vec<u8> = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                HirTemplatePart::Str(s) => {
                    let idx = self.chunk.add_str(s);
                    let r = self.alloc();
                    self.chunk.emit_rc(OpCode::LoadConst, r, idx, self.line);
                    part_regs.push(r);
                }
                HirTemplatePart::Expr(e) => {
                    let r = self.lower_expr(e);
                    self.chunk.emit_rr(OpCode::ToString, r, r, self.line);
                    part_regs.push(r);
                }
            }
        }
        let count = part_regs.len() as u8;
        self.chunk
            .write(Chunk::pack_op(OpCode::BuildStr, dest), self.line);
        self.chunk.write(Chunk::pack(count, 0), self.line);
        for &reg in &part_regs {
            self.chunk.write(Chunk::pack(reg, 0), self.line);
        }
        self.free_to(dest as u32 + 1); // result in dest; drop the part temps
        dest
    }

    /// Assignment as an expression: perform the store and yield the assigned
    /// value (collapsed down to the entry temp to keep the net-`+1` contract).
    fn lower_assign_expr(&mut self, target: &HirAssignTarget, value: &HirExpr) -> u8 {
        match target {
            HirAssignTarget::Var(binding) => {
                let v = self.lower_expr(value);
                self.store_binding(binding, v);
                v
            }
            HirAssignTarget::Member { object, name } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let val = self.lower_expr(value);
                let key_idx = self.chunk.add_str(name);
                self.emit_property(OpCode::SetProperty, obj, val, key_idx);
                self.chunk.emit_rr(OpCode::Move, mark as u8, val, self.line);
                self.free_to(mark + 1);
                mark as u8
            }
            HirAssignTarget::Index {
                object,
                index,
            } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let idx = self.lower_expr(index);
                let val = self.lower_expr(value);
                self.chunk
                    .emit_rrr(OpCode::SetIndex, obj, idx, val, self.line);
                self.chunk.emit_rr(OpCode::Move, mark as u8, val, self.line);
                self.free_to(mark + 1);
                mark as u8
            }
            HirAssignTarget::ModuleSlot { slot } => {
                let val = self.lower_expr(value);
                self.chunk.emit_rc(OpCode::StoreModuleSlot, val, *slot, self.line);
                val
            }
            HirAssignTarget::SuperMember { name } => {
                let mark = self.next_temp;
                let ctor_idx = self.chunk.add_str("constructor");
                let obj = self.alloc();
                self.chunk.emit_rc(OpCode::GetSuper, obj, ctor_idx, self.line);
                let val = self.lower_expr(value);
                let key_idx = self.chunk.add_str(name);
                self.emit_property(OpCode::SetProperty, obj, val, key_idx);
                self.chunk.emit_rr(OpCode::Move, mark as u8, val, self.line);
                self.free_to(mark + 1);
                mark as u8
            }
            HirAssignTarget::SuperIndex { index } => {
                let mark = self.next_temp;
                let ctor_idx = self.chunk.add_str("constructor");
                let obj = self.alloc();
                self.chunk.emit_rc(OpCode::GetSuper, obj, ctor_idx, self.line);
                let idx = self.lower_expr(index);
                let val = self.lower_expr(value);
                self.chunk.emit_rrr(OpCode::SetIndex, obj, idx, val, self.line);
                self.chunk.emit_rr(OpCode::Move, mark as u8, val, self.line);
                self.free_to(mark + 1);
                mark as u8
            }
        }
    }

    /// Object literal → `BuildObject` (explicit key/value pairs). Each value reg
    /// is listed in the bytecode, so `regalloc_post` tracks and remaps them
    /// independently — unlike `BuildObjectWithShape`, whose value block must stay
    /// register-contiguous (its count lives in the shape constant, invisible to
    /// the bytecode-only `regalloc_post`, so it can mis-track values that aren't
    /// the last temps; legacy avoids this via its IR liveness). Mirrors legacy
    /// `compile_object`'s general (`BuildObject`) path. Shape-sharing via
    /// `BuildObjectWithShape` is a deferred optimisation (needs regalloc support).
    fn lower_object(&mut self, properties: &[HirObjectProp]) -> u8 {
        let dest = self.alloc();
        let mut pending_pairs: Vec<(u16, u8)> = Vec::new();
        let mut first_segment = true;
        let mut mark = self.next_temp;

        for prop in properties {
            match prop {
                HirObjectProp::Property { key, value } => {
                    let key_idx = match key {
                        HirPropKey::Static(s) => self.chunk.add_str(s),
                        HirPropKey::Computed(e) => {
                            let r = self.lower_expr(e);
                            let str_r = self.alloc();
                            self.chunk.emit_rr(OpCode::ToString, str_r, r, self.line);
                            let s = format!("__computed_{}__", str_r);
                            self.chunk.add_str(&s)
                        }
                    };
                    let val_reg = self.lower_expr(value);
                    pending_pairs.push((key_idx, val_reg));
                }
                HirObjectProp::Method { key, func, upvalues } => {
                    let key_str = match key {
                        HirPropKey::Static(s) => s.to_string(),
                        HirPropKey::Computed(_) => "<computed>".to_owned(),
                    };
                    let key_idx = self.chunk.add_str(&key_str);
                    let closure_reg = self.lower_closure(func, upvalues);
                    pending_pairs.push((key_idx, closure_reg));
                }
                HirObjectProp::Spread(argument) => {
                    if first_segment {
                        let count = pending_pairs.len() as u8;
                        self.chunk.emit(OpCode::BuildObject, self.line);
                        self.chunk.write(Chunk::pack(dest, count), self.line);
                        for (key_idx, val_reg) in &pending_pairs {
                            self.chunk.write(*key_idx, self.line);
                            self.chunk.write(Chunk::pack(*val_reg, 0), self.line);
                        }
                        self.free_to(mark);
                        pending_pairs.clear();
                        first_segment = false;
                    } else if !pending_pairs.is_empty() {
                        let tmp = self.alloc();
                        let count = pending_pairs.len() as u8;
                        self.chunk.emit(OpCode::BuildObject, self.line);
                        self.chunk.write(Chunk::pack(tmp, count), self.line);
                        for (key_idx, val_reg) in &pending_pairs {
                            self.chunk.write(*key_idx, self.line);
                            self.chunk.write(Chunk::pack(*val_reg, 0), self.line);
                        }
                        self.chunk.emit_rr(OpCode::ObjectMerge, dest, tmp, self.line);
                        self.free_to(mark);
                        pending_pairs.clear();
                    }
                    let src = self.lower_expr(argument);
                    self.chunk.emit_rr(OpCode::ObjectMerge, dest, src, self.line);
                    self.free();
                    mark = self.next_temp;
                }
            }
        }

        if first_segment {
            let count = pending_pairs.len() as u8;
            self.chunk.emit(OpCode::BuildObject, self.line);
            self.chunk.write(Chunk::pack(dest, count), self.line);
            for (key_idx, val_reg) in &pending_pairs {
                self.chunk.write(*key_idx, self.line);
                self.chunk.write(Chunk::pack(*val_reg, 0), self.line);
            }
            self.free_to(mark);
        } else if !pending_pairs.is_empty() {
            let tmp = self.alloc();
            let count = pending_pairs.len() as u8;
            self.chunk.emit(OpCode::BuildObject, self.line);
            self.chunk.write(Chunk::pack(tmp, count), self.line);
            for (key_idx, val_reg) in &pending_pairs {
                self.chunk.write(*key_idx, self.line);
                self.chunk.write(Chunk::pack(*val_reg, 0), self.line);
            }
            self.chunk.emit_rr(OpCode::ObjectMerge, dest, tmp, self.line);
            self.free_to(mark);
        }

        dest
    }

    /// Closure value: lower the nested function to a proto constant, then emit
    /// `MakeClosure` (or `LoadStaticFn` when it captures nothing). Upvalue
    /// sources are resolved to `(is_local, index)` against this (parent) frame's
    /// register layout. Mirrors legacy `emit_closure`.
    pub(super) fn lower_closure(&mut self, func: &HirFunction, upvalues: &[HirUpvalueSrc]) -> u8 {
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
