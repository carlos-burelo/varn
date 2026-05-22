use super::compiler::{Compiler, UpvalueDesc};
use super::stmt::compile_stmts;
use crate::chunk::{Chunk, FunctionProto, Literal, PoolEntry};
use std::rc::Rc;
use varn_core::ast::*;
use varn_core::OpCode;

pub fn compile_function<'a>(
    parent: &mut Compiler<'a>,
    name: Rc<str>,
    params: &[Param],
    body: &Stmt,
    is_async: bool,
    is_generator: bool,
    has_this: bool,
) -> (FunctionProto, Vec<UpvalueDesc>) {
    let mut child = Compiler::new_function(
        name,
        parent.source_file.clone(),
        is_async,
        is_generator,
        has_this,
        parent.annotations,
        parent.extension_calls,
        parent.extension_members,
        parent.extension_set_members,
        parent.protos.clone(),
        parent as *mut Compiler<'a>,
        parent.parent_depth + 1,
        parent.current_class.clone(),
        parent.current_superclass.clone(),
    );

    let recv_reg = child.alloc_reg();
    assert_eq!(recv_reg, 0, "First allocated register must be 0 for receiver");
    if has_this {
        child.define_local(Rc::from("this"), 0);
    }

    let mut arity = 1usize;

    let has_rest = params.iter().any(|p| p.is_rest);
    child.has_rest = has_rest;

    let param_regs: Vec<u8> = params
        .iter()
        .map(|param| {
            let r = child.alloc_reg();
            let name = match &param.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => Rc::from(format!("__p{}", arity)),
            };
            child.define_local(name, r);
            arity += 1;
            r
        })
        .collect();

    child.arity = arity;

    for (i, param) in params.iter().enumerate() {
        child.line = param.range.start.line;

        if let Some(default_expr) = &param.default {
            let is_null = child.alloc_reg();
            child.emit_rr(OpCode::IsNull, is_null, param_regs[i]);
            let skip = child.emit_cond_jump(OpCode::JumpIfFalse, is_null);
            child.free_reg();
            let default_reg = super::expr::compile_expr(&mut child, default_expr);
            child.emit_rr(OpCode::Move, param_regs[i], default_reg);
            child.free_reg();
            child.patch_jump(skip);
        }
        match &param.pattern {
            Pattern::Identifier { .. } => {}
            other => {
                declare_pattern_into(&mut child, other, param_regs[i]);
            }
        }
    }

    if &*child.name == "constructor" {
        child.pending_field_inits = std::mem::take(&mut parent.pending_field_inits);
    }

    match &body.kind {
        StmtKind::Block { stmts } => compile_stmts(&mut child, stmts),
        _ => super::stmt::compile_stmt(&mut child, body),
    }

    if &*child.name == "constructor" {
        super::expr::emit_field_inits(&mut child);
    }

    let line = child.line;
    let ret = child.alloc_reg();
    child.chunk.emit_rr(OpCode::LoadNull, ret, 0, line);
    child
        .chunk
        .emit1(OpCode::Return, Chunk::pack(0, ret) as u16, line);

    child.finish_function()
}

pub fn emit_closure<'a>(
    parent: &mut Compiler<'a>,
    proto: FunctionProto,
    upvalues: Vec<UpvalueDesc>,
) -> u8 {
    let proto_idx = parent.add_const(PoolEntry::Function(Rc::new(proto)));
    let dest = parent.alloc_reg();
    let uv_count = upvalues.len() as u8;

    let line = parent.line;
    parent.chunk.emit(OpCode::MakeClosure, line);
    parent.chunk.write(Chunk::pack(dest, uv_count as u8), line);
    parent.chunk.write(proto_idx, line);
    for uv in &upvalues {
        parent.chunk.write(
            Chunk::pack(if uv.is_local { 1 } else { 0 }, uv.index as u8),
            line,
        );
    }
    dest
}

pub fn declare_pattern_into<'a>(c: &mut Compiler<'a>, pattern: &Pattern, src_reg: u8) {
    match pattern {
        Pattern::Identifier { name, .. } => {
            if let Some(loc) = c.resolve_local(name) {
                use super::compiler::VarLoc;
                if let VarLoc::Reg(dest) | VarLoc::Captured(dest) = loc {
                    if dest != src_reg {
                        c.emit_rr(OpCode::Move, dest, src_reg);
                    }
                    return;
                }
            }

            c.define_local(name.clone(), src_reg);
        }

        Pattern::Array { elements, rest, .. } => {
            for (i, el) in elements.iter().enumerate() {
                if let Some(elem) = el {
                    let dest = c.alloc_reg();
                    let key_idx = c.add_const(PoolEntry::Literal(Literal::Int(i as i64)));
                    let key_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadConst, key_reg, key_idx);
                    c.emit_rrr(OpCode::GetIndex, dest, src_reg, key_reg);
                    c.free_reg();
                    declare_pattern_into(c, &elem.pattern, dest);
                }
            }
            if let Some(rest_pat) = rest {
                let dest = c.alloc_reg();
                let start_idx =
                    c.add_const(PoolEntry::Literal(Literal::Int(elements.len() as i64)));
                let slice_key = c.add_str("slice");
                let cs = c.alloc_cache() as u8;
                let arg = c.alloc_reg();

                c.emit_rc(OpCode::LoadConst, arg, start_idx);
                let line = c.line;

                c.chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
                c.chunk.write(Chunk::pack(dest, src_reg), line);
                c.chunk.write(slice_key, line);
                c.chunk.write(Chunk::pack(1, arg), line);
                c.free_reg();
                declare_pattern_into(c, rest_pat, dest);
            }
        }

        Pattern::Object {
            properties, rest, ..
        } => {
            for prop in properties {
                let dest = c.alloc_reg();
                let key_idx = c.add_str(&prop.key);
                c.emit_rrc(OpCode::GetPropertyMaybe, dest, src_reg, key_idx);
                declare_pattern_into(c, &prop.value, dest);
            }
            if let Some(rest_pat) = rest {
                let skip_count = properties.len() as u8;
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::ObjectRest, line);
                c.chunk.write(Chunk::pack(dest, src_reg), line);
                c.chunk.write(Chunk::pack(skip_count, 0), line);
                for prop in properties {
                    let key_idx = c.add_str(&prop.key);
                    c.chunk.write(key_idx, line);
                }
                declare_pattern_into(c, rest_pat, dest);
            }
        }

        Pattern::Assignment { left, right, .. } => {
            let is_null = c.alloc_reg();
            c.emit_rr(OpCode::IsNull, is_null, src_reg);
            let skip = c.emit_cond_jump(OpCode::JumpIfFalse, is_null);
            c.free_reg();

            let default_reg = super::expr::compile_expr(c, right);
            c.emit_rr(OpCode::Move, src_reg, default_reg);
            c.free_reg();

            c.patch_jump(skip);
            declare_pattern_into(c, left, src_reg);
        }

        Pattern::Rest { argument, .. } => {
            declare_pattern_into(c, argument, src_reg);
        }
    }
}

pub fn declare_pattern_local<'a>(c: &mut Compiler<'a>, pattern: &Pattern, src_reg: u8) {
    declare_pattern_into(c, pattern, src_reg);
}

pub fn declare_pattern_global<'a>(c: &mut Compiler<'a>, pattern: &Pattern, src_reg: u8) {
    use crate::chunk::Literal;
    match pattern {
        Pattern::Identifier { name, .. } => {
            let idx = c.add_str(name);
            c.emit_rrc(OpCode::DefineGlobal, 0, src_reg, idx);
        }
        Pattern::Array { elements, rest, .. } => {
            for (i, el) in elements.iter().enumerate() {
                if let Some(elem) = el {
                    let dest = c.alloc_reg();
                    let key_idx = c.add_const(PoolEntry::Literal(Literal::Int(i as i64)));
                    let key_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadConst, key_reg, key_idx);
                    c.emit_rrr(OpCode::GetIndex, dest, src_reg, key_reg);
                    c.free_reg();
                    declare_pattern_global(c, &elem.pattern, dest);
                    c.free_reg();
                }
            }
            if let Some(rest_pat) = rest {
                let dest = c.alloc_reg();
                let start_idx =
                    c.add_const(PoolEntry::Literal(Literal::Int(elements.len() as i64)));
                let slice_key = c.add_str("slice");
                let cs = c.alloc_cache() as u8;
                let arg = c.alloc_reg();
                c.emit_rc(OpCode::LoadConst, arg, start_idx);
                let line = c.line;

                c.chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
                c.chunk.write(Chunk::pack(dest, src_reg), line);
                c.chunk.write(slice_key, line);
                c.chunk.write(Chunk::pack(1, arg), line);
                c.free_reg();
                declare_pattern_global(c, rest_pat, dest);
                c.free_reg();
            }
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            for prop in properties {
                let dest = c.alloc_reg();
                let key_idx = c.add_str(&prop.key);
                c.emit_rrc(OpCode::GetPropertyMaybe, dest, src_reg, key_idx);
                declare_pattern_global(c, &prop.value, dest);
                c.free_reg();
            }
            if let Some(rest_pat) = rest {
                let skip_count = properties.len() as u8;
                let dest = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::ObjectRest, line);
                c.chunk.write(Chunk::pack(dest, src_reg), line);
                c.chunk.write(Chunk::pack(skip_count, 0), line);
                for prop in properties {
                    let key_idx = c.add_str(&prop.key);
                    c.chunk.write(key_idx, line);
                }
                declare_pattern_global(c, rest_pat, dest);
                c.free_reg();
            }
        }
        Pattern::Assignment { left, right, .. } => {
            let is_null = c.alloc_reg();
            c.emit_rr(OpCode::IsNull, is_null, src_reg);
            let skip = c.emit_cond_jump(OpCode::JumpIfFalse, is_null);
            c.free_reg();
            let default_reg = super::expr::compile_expr(c, right);
            c.emit_rr(OpCode::Move, src_reg, default_reg);
            c.free_reg();
            c.patch_jump(skip);
            declare_pattern_global(c, left, src_reg);
        }
        Pattern::Rest { argument, .. } => {
            declare_pattern_global(c, argument, src_reg);
        }
    }
}
