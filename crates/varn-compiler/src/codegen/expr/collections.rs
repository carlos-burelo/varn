use super::super::compiler::Compiler;
use crate::chunk::{Chunk, PoolEntry};
use std::rc::Rc;
use varn_core::ast::expr::{ObjectProp, PropKey};
use varn_core::ast::ArrayEl;
use varn_core::OpCode;

use super::super::function::{compile_function, emit_closure};
use super::compile_expr;

pub(super) fn compile_array<'a>(c: &mut Compiler<'a>, elements: &[ArrayEl]) -> u8 {
    let has_spread = elements.iter().any(|e| matches!(e, ArrayEl::Spread(_)));

    if !has_spread {
        let count = elements.len() as u8;

        let dest = c.alloc_reg();
        let start = c.regs.next as u8;
        for el in elements {
            match el {
                ArrayEl::Hole => {
                    let r = c.alloc_reg();
                    c.emit_rr(OpCode::LoadNull, r, 0);
                    let _ = r;
                }
                ArrayEl::Expr(e) => {
                    let _ = compile_expr(c, e);
                }
                ArrayEl::Spread(_) => unreachable!(),
            };
        }
        let line = c.line;
        c.chunk.emit(OpCode::BuildArray, line);
        c.chunk.write(Chunk::pack(dest, start), line);
        c.chunk.write(Chunk::pack(count, 0), line);

        for _ in 0..count {
            c.free_reg();
        }
        return dest;
    }

    let arr = c.alloc_reg();
    let line = c.line;
    c.chunk.emit(OpCode::BuildArray, line);
    c.chunk.write(Chunk::pack(arr, 0), line);
    c.chunk.write(Chunk::pack(0, 0), line);

    for el in elements {
        match el {
            ArrayEl::Hole => {
                let null_r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, null_r, 0);
                c.emit_rr(OpCode::ArrayPush, arr, null_r);
                c.free_reg();
            }
            ArrayEl::Expr(e) => {
                let r = compile_expr(c, e);
                c.emit_rr(OpCode::ArrayPush, arr, r);
                c.free_reg();
            }
            ArrayEl::Spread(e) => {
                let r = compile_expr(c, e);
                c.emit_rr(OpCode::ArrayExtend, arr, r);
                c.free_reg();
            }
        }
    }
    arr
}

pub(super) fn compile_object<'a>(c: &mut Compiler<'a>, properties: &[ObjectProp]) -> u8 {
    let mut keys: Vec<Rc<str>> = Vec::new();
    let mut is_simple = true;
    for prop in properties {
        match prop {
            ObjectProp::Property { key, .. } => match key {
                PropKey::Identifier(s) | PropKey::Str(s) => keys.push(Rc::from(s.as_str())),
                PropKey::Int(n) => keys.push(Rc::from(n.to_string().as_str())),
                PropKey::Computed(_) => {
                    is_simple = false;
                    break;
                }
            },
            _ => {
                is_simple = false;
                break;
            }
        }
    }

    if is_simple && !keys.is_empty() {
        let shape_idx = c.add_const(PoolEntry::Shape(keys));
        let count = properties.len() as u8;

        let dest = c.alloc_reg();
        let start = c.regs.next as u8;
        for prop in properties {
            if let ObjectProp::Property { value, .. } = prop {
                let _ = compile_expr(c, value);
            }
        }
        let line = c.line;
        c.chunk.emit(OpCode::BuildObjectWithShape, line);
        c.chunk.write(Chunk::pack(dest, start), line);
        c.chunk.write(shape_idx, line);
        for _ in 0..count {
            c.free_reg();
        }
        return dest;
    }

    let dest = c.alloc_reg();
    let mut pending_pairs: Vec<(u16, u8)> = Vec::new();

    let flush_segment = |c: &mut Compiler<'a>, dest: u8, pairs: &mut Vec<(u16, u8)>| {
        let count = pairs.len() as u8;
        let line = c.line;
        c.chunk.emit(OpCode::BuildObject, line);
        c.chunk.write(Chunk::pack(dest, count), line);
        for (key_idx, val_reg) in pairs.iter() {
            c.chunk.write(*key_idx, line);
            c.chunk.write(Chunk::pack(*val_reg as u8, 0), line);
        }
        for _ in pairs.iter() {
            c.free_reg();
        }
        pairs.clear();
    };

    let mut first_segment = true;

    for prop in properties {
        match prop {
            ObjectProp::Property { key, value, .. } => {
                let key_idx = match key {
                    PropKey::Identifier(s) | PropKey::Str(s) => c.add_str(s),
                    PropKey::Int(n) => {
                        let s = n.to_string();
                        c.add_str(&s)
                    }
                    PropKey::Computed(e) => {
                        let r = compile_expr(c, e);
                        let str_r = c.alloc_reg();
                        c.emit_rr(OpCode::ToString, str_r, r);
                        c.free_reg();

                        let s = format!("__computed_{}__", str_r);
                        c.add_str(&s)
                    }
                };
                let val_reg = compile_expr(c, value);
                pending_pairs.push((key_idx, val_reg));
            }
            ObjectProp::Method {
                key,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                let key_str = match key {
                    PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
                    PropKey::Int(n) => n.to_string(),
                    PropKey::Computed(_) => "<computed>".to_owned(),
                };
                let key_idx = c.add_str(&key_str);
                let (proto, upvalues) = compile_function(
                    c,
                    Rc::from(key_str),
                    params,
                    body,
                    *is_async,
                    *is_generator,
                    true,
                );
                let closure_reg = emit_closure(c, proto, upvalues, 0);
                pending_pairs.push((key_idx, closure_reg));
            }
            ObjectProp::Spread { argument, .. } => {
                if first_segment {
                    flush_segment(c, dest, &mut pending_pairs);
                    first_segment = false;
                } else if !pending_pairs.is_empty() {
                    let tmp = c.alloc_reg();
                    flush_segment(c, tmp, &mut pending_pairs);
                    c.emit_rr(OpCode::ObjectMerge, dest, tmp);
                    c.free_reg();
                }
                let src = compile_expr(c, argument);
                c.emit_rr(OpCode::ObjectMerge, dest, src);
                c.free_reg();
            }
            _ => {}
        }
    }

    if first_segment {
        flush_segment(c, dest, &mut pending_pairs);
    } else if !pending_pairs.is_empty() {
        let tmp = c.alloc_reg();
        flush_segment(c, tmp, &mut pending_pairs);
        c.emit_rr(OpCode::ObjectMerge, dest, tmp);
        c.free_reg();
    }

    dest
}
