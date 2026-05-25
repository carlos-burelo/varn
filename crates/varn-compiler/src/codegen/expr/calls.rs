use super::super::compiler::Compiler;
use crate::chunk::Chunk;
use varn_core::ast::expr::Arg;
use varn_core::ast::{Expr, ExprKind};
use varn_core::OpCode;

use super::compile_expr;
use super::fields::emit_field_inits;

pub(super) fn compile_call<'a>(
    c: &mut Compiler<'a>,
    callee: &Expr,
    args: &[Arg],
    optional: bool,
    offset: u32,
) -> u8 {
    if let Some(mangled) = c.extension_calls.get(&offset).cloned() {
        if let ExprKind::Member {
            object,
            optional: mem_opt,
            ..
        } = &callee.kind
        {
            let obj = compile_expr(c, object);
            if optional || *mem_opt {
                let is_null = c.alloc_reg();
                c.emit_rr(OpCode::IsNull, is_null, obj);
                let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                c.free_reg();
                let dest = emit_extension_call(c, &mangled, obj, offset, args);
                c.free_reg();
                c.patch_jump(end);
                return dest;
            } else {
                let dest = emit_extension_call(c, &mangled, obj, offset, args);
                c.free_reg();
                return dest;
            }
        }
    }

    if optional {
        let callee_reg = compile_expr(c, callee);
        let is_null = c.alloc_reg();
        c.emit_rr(OpCode::IsNull, is_null, callee_reg);
        let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.free_reg();
        let dest = emit_plain_call(c, callee_reg, offset, args);

        c.patch_jump(end);
        return dest;
    }

    if let ExprKind::Member {
        object,
        property,
        computed,
        optional: mem_opt,
    } = &callee.kind
    {
        if !computed {
            if let ExprKind::Identifier { name } = &property.as_ref().kind {
                if matches!(&object.as_ref().kind, ExprKind::Super) {
                    let super_idx = c.add_str(name);
                    let fn_reg = c.alloc_reg();
                    c.emit_rc(OpCode::GetSuper, fn_reg, super_idx);
                    let (arg_start, arg_count, has_spread) =
                        compile_args_contiguous(c, offset, args);

                    let dest = fn_reg;
                    let line = c.line;
                    if has_spread {
                        c.chunk.emit(OpCode::CallSpread, line);
                        c.chunk.write(Chunk::pack(dest, fn_reg), line);
                        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
                    } else {
                        c.chunk.emit(OpCode::Call, line);
                        c.chunk.write(Chunk::pack(dest, fn_reg), line);
                        c.chunk.write(
                            Chunk::pack(arg_count as u8, if arg_count > 0 { arg_start } else { 0 }),
                            line,
                        );
                    }
                    for _ in 0..arg_count {
                        c.free_reg();
                    }
                    if name.as_ref() == "constructor" {
                        emit_field_inits(c);
                    }
                    return dest;
                }

                if *mem_opt {
                    let obj = compile_expr(c, object);
                    let is_null = c.alloc_reg();
                    c.emit_rr(OpCode::IsNull, is_null, obj);
                    let end = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
                    c.free_reg();
                    let dest = emit_method_call(c, obj, name, offset, args);

                    c.patch_jump(end);
                    return dest;
                }

                let obj = compile_expr(c, object);
                let dest = emit_method_call(c, obj, name, offset, args);

                return dest;
            }
        }
    }

    if let ExprKind::Super = &callee.kind {
        let super_idx = c.add_str("constructor");
        let fn_reg = c.alloc_reg();
        c.emit_rc(OpCode::GetSuper, fn_reg, super_idx);
        let line = c.line;
        let recv_reg = c.alloc_reg();
        c.emit_rr(OpCode::Move, recv_reg, 0);

        let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);
        assert_eq!(
            arg_start,
            recv_reg + 1,
            "contiguous args must start right after receiver register"
        );

        let dest = fn_reg;
        if has_spread {
            c.chunk.emit(OpCode::CallSpread, line);
            c.chunk.write(Chunk::pack(dest, fn_reg), line);
            c.chunk
                .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
        } else {
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(dest, fn_reg), line);
            c.chunk
                .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
        }
        for _ in 0..arg_count {
            c.free_reg();
        }
        c.free_reg();
        if &*c.name == "constructor" {
            emit_field_inits(c);
        }
        return dest;
    }

    let callee_reg = compile_expr(c, callee);

    emit_plain_call(c, callee_reg, offset, args)
}

pub(super) fn emit_method_call<'a>(
    c: &mut Compiler<'a>,
    obj: u8,
    name: &str,
    offset: u32,
    args: &[Arg],
) -> u8 {
    let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);

    let dest = obj;
    let str_idx = c.add_str(name);
    let cs = c.alloc_cache() as u8;
    let line = c.line;
    if has_spread {
        let fn_reg = c.alloc_reg();
        c.emit_property(OpCode::GetProperty, fn_reg, obj, str_idx);
        c.chunk.emit(OpCode::CallSpread, line);
        c.chunk.write(Chunk::pack(dest, fn_reg), line);
        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
        c.free_reg();
    } else {
        c.chunk.write(Chunk::pack_op(OpCode::CallMethod, cs), line);
        c.chunk.write(Chunk::pack(dest, obj), line);
        c.chunk.write(str_idx, line);
        c.chunk.write(Chunk::pack(arg_count as u8, arg_start), line);
    }
    for _ in 0..arg_count {
        c.free_reg();
    }
    dest
}

pub(super) fn emit_plain_call<'a>(
    c: &mut Compiler<'a>,
    callee_reg: u8,
    offset: u32,
    args: &[Arg],
) -> u8 {
    let line = c.line;
    let recv_reg = c.alloc_reg();
    c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

    let (arg_start, arg_count, has_spread) = compile_args_contiguous(c, offset, args);
    assert_eq!(
        arg_start,
        recv_reg + 1,
        "contiguous args must start right after receiver register"
    );

    let dest = callee_reg;
    if has_spread {
        c.chunk.emit(OpCode::CallSpread, line);
        c.chunk.write(Chunk::pack(dest, callee_reg), line);
        c.chunk
            .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
    } else {
        c.chunk.emit(OpCode::Call, line);
        c.chunk.write(Chunk::pack(dest, callee_reg), line);
        c.chunk
            .write(Chunk::pack((arg_count + 1) as u8, recv_reg), line);
    }
    for _ in 0..arg_count {
        c.free_reg();
    }
    c.free_reg();
    dest
}

fn emit_extension_call<'a>(
    c: &mut Compiler<'a>,
    mangled: &str,
    obj: u8,
    offset: u32,
    args: &[Arg],
) -> u8 {
    let fn_reg = c.alloc_reg();
    let idx = c.add_str(mangled);
    c.emit_rc(OpCode::LoadGlobal, fn_reg, idx);

    let obj_copy = c.alloc_reg();
    c.emit_rr(OpCode::Move, obj_copy, obj);
    let (_arg_start, arg_count, _) = compile_args_contiguous(c, offset, args);

    let dest = c.alloc_reg();
    let line = c.line;
    c.chunk.emit(OpCode::Call, line);
    c.chunk.write(Chunk::pack(dest, fn_reg), line);
    c.chunk
        .write(Chunk::pack((arg_count + 1) as u8, obj_copy), line);
    for _ in 0..arg_count {
        c.free_reg();
    }
    c.free_reg();
    c.free_reg();
    dest
}

pub fn compile_args_contiguous<'a>(
    c: &mut Compiler<'a>,
    call_id: u32,
    args: &[Arg],
) -> (u8, usize, bool) {
    let start = c.regs.next;
    let mut count = 0usize;
    let mut has_spread = false;

    let mapping = c.annotations.get_call_mapping(call_id).cloned();

    if let Some(mapping) = mapping {
        for opt_idx in &mapping {
            if let Some(arg_idx) = opt_idx {
                match &args[*arg_idx] {
                    Arg::Positional(e) | Arg::Named { value: e, .. } => {
                        let r = compile_expr(c, e);
                        let slot = start + count as u8;
                        if r != slot {
                            while c.regs.next <= slot {
                                c.regs.alloc();
                            }
                            c.emit_rr(OpCode::Move, slot, r);
                        }
                    }
                    Arg::Spread(e) => {
                        let r = compile_expr(c, e);
                        let slot = start + count as u8;
                        if r != slot {
                            while c.regs.next <= slot {
                                c.regs.alloc();
                            }
                            c.emit_rr(OpCode::WrapSpread, slot, r);
                        } else {
                            c.emit_rr(OpCode::WrapSpread, r, r);
                        }
                        has_spread = true;
                    }
                }
            } else {
                let slot = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, slot, 0);
            }
            count += 1;
        }
    } else {
        for arg in args {
            match arg {
                Arg::Positional(e) | Arg::Named { value: e, .. } => {
                    let r = compile_expr(c, e);
                    let slot = start + count as u8;
                    if r != slot {
                        while c.regs.next <= slot {
                            c.regs.alloc();
                        }
                        c.emit_rr(OpCode::Move, slot, r);
                    }
                }
                Arg::Spread(e) => {
                    let r = compile_expr(c, e);
                    let slot = start + count as u8;
                    if r != slot {
                        while c.regs.next <= slot {
                            c.regs.alloc();
                        }
                        c.emit_rr(OpCode::WrapSpread, slot, r);
                    } else {
                        c.emit_rr(OpCode::WrapSpread, r, r);
                    }
                    has_spread = true;
                }
            }
            count += 1;
        }
    }

    (start, count, has_spread)
}
