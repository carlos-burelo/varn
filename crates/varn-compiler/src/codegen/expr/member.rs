use super::super::compiler::Compiler;
use varn_core::ast::{Expr, ExprKind};
use varn_core::OpCode;

use super::compile_expr;

pub(super) fn compile_is<'a>(
    c: &mut Compiler<'a>,
    expression: &Expr,
    type_ann: &varn_core::ast::types::TypeNode,
) -> u8 {
    use varn_core::{IntrinsicType, TypeKind, TypeTag};
    let src = compile_expr(c, expression);
    let dest = c.alloc_reg();
    match &type_ann.kind {
        TypeKind::Intrinsic(TypeTag::Null) => {
            c.emit_rr(OpCode::IsNull, dest, src);
        }
        TypeKind::Intrinsic(TypeTag::Array) | TypeKind::Array(_) => {
            c.emit_rr(OpCode::IsArray, dest, src);
        }
        TypeKind::Generic(n, _, _) if n == IntrinsicType::Array.as_str() => {
            c.emit_rr(OpCode::IsArray, dest, src);
        }
        TypeKind::Intrinsic(tt) => {
            let type_str = c.alloc_reg();
            c.emit_rr(OpCode::Typeof, type_str, src);
            let it = IntrinsicType::from(*tt).as_str();
            let s_idx = c.add_str(it);
            let s_reg = c.alloc_reg();
            c.emit_rc(OpCode::LoadConst, s_reg, s_idx);
            c.emit_rrr(OpCode::Eq, dest, type_str, s_reg);
            c.free_reg();
            c.free_reg();
        }
        TypeKind::Named(name, _) => {
            if let Some(it) = IntrinsicType::from_str(name) {
                if it.is_scalar_primitive() {
                    let type_str = c.alloc_reg();
                    c.emit_rr(OpCode::Typeof, type_str, src);
                    let s_idx = c.add_str(it.as_str());
                    let s_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadConst, s_reg, s_idx);
                    c.emit_rrr(OpCode::Eq, dest, type_str, s_reg);
                    c.free_reg();
                    c.free_reg();
                } else {
                    let cls = c.alloc_reg();
                    let idx = c.add_str(name);
                    c.emit_rc(OpCode::LoadGlobal, cls, idx);
                    c.emit_rrr(OpCode::Instanceof, dest, src, cls);
                    c.free_reg();
                }
            } else {
                let cls = c.alloc_reg();
                let idx = c.add_str(name);
                c.emit_rc(OpCode::LoadGlobal, cls, idx);
                c.emit_rrr(OpCode::Instanceof, dest, src, cls);
                c.free_reg();
            }
        }
        _ => {
            c.emit_rr(OpCode::LoadFalse, dest, 0);
        }
    }
    c.free_reg();
    dest
}

pub(super) fn compile_member_access<'a>(
    c: &mut Compiler<'a>,
    object: &Expr,
    property: &Expr,
    computed: bool,
    optional: bool,
    offset: u32,
) -> u8 {
    if !computed {
        if let Some(slot_idx) = c.annotations.get_slot_idx(offset) {
            let obj = compile_expr(c, object);
            if optional {
                let is_null = c.alloc_reg();
                c.emit_rr(OpCode::IsNull, is_null, obj);
                let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                c.free_reg();
                c.emit_rrc(OpCode::LoadModuleSlot, obj, obj, slot_idx as u16);
                c.patch_jump(end);
            } else {
                c.emit_rrc(OpCode::LoadModuleSlot, obj, obj, slot_idx as u16);
            }
            return obj;
        }

        if let Some(mangled) = c.extension_members.get(&offset).cloned() {
            let obj = compile_expr(c, object);
            if optional {
                let is_null = c.alloc_reg();
                c.emit_rr(OpCode::IsNull, is_null, obj);
                let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                c.free_reg();
                let setter = c.alloc_reg();
                let idx = c.add_str(&mangled);
                c.emit_rc(OpCode::LoadGlobal, setter, idx);
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(crate::chunk::Chunk::pack(dest, setter), line);
                c.chunk.write(crate::chunk::Chunk::pack(1, obj), line);
                c.free_reg();
                c.free_reg();
                c.patch_jump(end);
                return dest;
            } else {
                let setter = c.alloc_reg();
                let idx = c.add_str(&mangled);
                c.emit_rc(OpCode::LoadGlobal, setter, idx);
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(crate::chunk::Chunk::pack(dest, setter), line);
                c.chunk.write(crate::chunk::Chunk::pack(1, obj), line);
                c.free_reg();
                c.free_reg();
                return dest;
            }
        }
    }

    let obj = compile_expr(c, object);

    if optional {
        let is_null = c.alloc_reg();
        c.emit_rr(OpCode::IsNull, is_null, obj);
        let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.free_reg();
        if computed {
            let key = compile_expr(c, property);
            c.emit_rrr(OpCode::GetIndex, obj, obj, key);
            c.free_reg();
        } else {
            let name = match &property.kind {
                ExprKind::Identifier { name } => name.clone(),
                _ => {
                    return obj;
                }
            };
            let idx = c.add_str(&name);
            c.emit_rrc(OpCode::GetPropertyMaybe, obj, obj, idx);
        }
        c.patch_jump(end);
        obj
    } else if computed {
        let key = compile_expr(c, property);
        c.emit_rrr(OpCode::GetIndex, obj, obj, key);
        c.free_reg();
        obj
    } else {
        let name = match &property.kind {
            ExprKind::Identifier { name } => name.clone(),
            _ => {
                return obj;
            }
        };
        let idx = c.add_str(&name);
        c.emit_property(OpCode::GetProperty, obj, obj, idx);
        obj
    }
}
