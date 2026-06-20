use super::compiler::Compiler;
use super::expr::compile_expr;
use super::function::{compile_function, emit_closure};
use crate::chunk::Chunk;
use std::rc::Rc;
use varn_core::ast::decl::*;
use varn_core::ast::expr::*;
use varn_core::ast::operators::{Modifiers, Visibility};
use varn_core::ast::*;
use varn_core::OpCode;

fn apply_class_decorators<'a>(c: &mut Compiler<'a>, class_reg: u8, decorators: &[Decorator]) {
    for deco in decorators.iter().rev() {
        let deco_reg = compile_expr(c, &deco.expression) as u8;
        let result = c.alloc_reg();
        let is_null = c.alloc_reg();
        let line = c.line;

        let recv_reg = c.alloc_reg();
        c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

        let arg_class_reg = c.alloc_reg();
        c.emit_rr(OpCode::Move, arg_class_reg, class_reg);

        c.chunk.emit(OpCode::Call, line);
        c.chunk.write(Chunk::pack(result, deco_reg), line);
        c.chunk.write(Chunk::pack(2, recv_reg), line);

        c.emit_rr(OpCode::IsNull, is_null, result);
        let skip = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.emit_rr(OpCode::Move, class_reg, result);
        c.patch_jump(skip);

        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
    }
}

fn apply_method_decorators<'a>(
    c: &mut Compiler<'a>,
    _class_reg: u8,
    method_reg: u8,
    key: &str,
    modifiers: &Modifiers,
    decorators: &[Decorator],
) {
    if decorators.is_empty() {
        return;
    }

    for deco in decorators.iter().rev() {
        let deco_fn = compile_expr(c, &deco.expression) as u8;

        let k_name = c.add_str("name");
        let v_name = c.add_str(key);
        let k_kind = c.add_str("kind");
        let v_kind = c.add_str("method");
        let k_static = c.add_str("isStatic");
        let k_private = c.add_str("isPrivate");

        let n_r = c.alloc_reg();
        c.emit_rc(OpCode::LoadConst, n_r, v_name);
        let kind_r = c.alloc_reg();
        c.emit_rc(OpCode::LoadConst, kind_r, v_kind);
        let static_r = c.alloc_reg();
        c.emit_rr(
            if modifiers.is_static {
                OpCode::LoadTrue
            } else {
                OpCode::LoadFalse
            },
            static_r,
            0,
        );
        let private_r = c.alloc_reg();
        let is_private = matches!(modifiers.visibility, Some(Visibility::Private));
        c.emit_rr(
            if is_private {
                OpCode::LoadTrue
            } else {
                OpCode::LoadFalse
            },
            private_r,
            0,
        );

        let ctx_reg = c.alloc_reg();
        let line = c.line;
        c.chunk.emit(OpCode::BuildObject, line);
        c.chunk.write(Chunk::pack(ctx_reg, 4), line);
        c.chunk.write(k_name, line);
        c.chunk.write(Chunk::pack(n_r, 0), line);
        c.chunk.write(k_kind, line);
        c.chunk.write(Chunk::pack(kind_r, 0), line);
        c.chunk.write(k_static, line);
        c.chunk.write(Chunk::pack(static_r, 0), line);
        c.chunk.write(k_private, line);
        c.chunk.write(Chunk::pack(private_r, 0), line);
        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();

        let recv_reg = c.alloc_reg();
        c.chunk.emit_rr(OpCode::LoadNull, recv_reg, 0, line);

        let a0 = c.alloc_reg();
        c.emit_rr(OpCode::Move, a0, method_reg);

        let a1 = c.alloc_reg();
        c.emit_rr(OpCode::Move, a1, ctx_reg);

        let result = c.alloc_reg();

        c.chunk.emit(OpCode::Call, line);
        c.chunk.write(Chunk::pack(result, deco_fn), line);
        c.chunk.write(Chunk::pack(3, recv_reg), line);

        let is_null = c.alloc_reg();
        c.emit_rr(OpCode::IsNull, is_null, result);
        let skip = c.emit_cond_jump(OpCode::JumpIfTrue, is_null);
        c.emit_rr(OpCode::Move, method_reg, result);
        c.patch_jump(skip);

        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
        c.free_reg();
    }
}

pub fn compile_class_expr<'a>(c: &mut Compiler<'a>, decl: &ClassDecl) -> u8 {
    let name = decl.id.clone().unwrap_or_else(|| "anonymous".into());
    let name_idx = c.add_str(&name);

    let class_reg = c.alloc_reg();

    if let Some(super_expr) = &decl.super_class {
        let super_reg = compile_expr(c, super_expr) as u8;
        c.emit_rrc(OpCode::MakeClass, class_reg, super_reg, name_idx);
        c.free_reg();
    } else {
        let line = c.line;
        c.chunk
            .emit_rrc(OpCode::MakeClass, class_reg, 0, name_idx, line);
    }

    let local_name: Rc<str> = Rc::from(format!("__class_{}__", name));
    c.define_local(local_name, class_reg);

    let saved_class = c.current_class.clone();
    let saved_superclass = c.current_superclass.clone();
    let saved_inits = std::mem::take(&mut c.pending_field_inits);

    c.current_class = Some(name.clone());
    if let Some(super_expr) = &decl.super_class {
        if let ExprKind::Identifier { name: sn } = &super_expr.kind {
            c.current_superclass = Some(sn.clone());
        }
    }

    for member in &decl.body {
        if let ClassMember::Property {
            key,
            init,
            modifiers,
            ..
        } = member
        {
            if modifiers.is_static {
                let val_reg: u8 = if let Some(expr) = init {
                    compile_expr(c, expr) as u8
                } else {
                    let r = c.alloc_reg();
                    c.emit_rr(OpCode::LoadNull, r, 0);
                    r
                };
                let key_idx = c.add_str(key);
                let line = c.line;
                c.chunk.emit(OpCode::DefineStatic, line);
                c.chunk.write(Chunk::pack(class_reg, val_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            } else {
                let key_idx = c.add_str(key);
                let line = c.line;
                c.chunk.emit(OpCode::DeclareField, line);
                c.chunk.write(Chunk::pack(class_reg, 0), line);
                c.chunk.write(key_idx, line);
                if let Some(expr) = init {
                    c.pending_field_inits.push((key.clone(), expr.clone()));
                }
            }
        }
    }

    let mut has_constructor = false;
    for member in &decl.body {
        match member {
            ClassMember::Constructor { params, body, .. } => {
                has_constructor = true;
                let (proto, upvalues) =
                    compile_function(c, "constructor".into(), params, body, false, false, true);
                let ctor_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str("constructor");
                let line = c.line;
                c.chunk.emit(OpCode::Method, line);
                c.chunk.write(Chunk::pack(class_reg, ctor_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Method {
                key,
                params,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    params,
                    body,
                    modifiers.is_async,
                    modifiers.is_generator,
                    !modifiers.is_static,
                );
                let method_reg = emit_closure(c, proto, upvalues, 0);
                apply_method_decorators(c, class_reg, method_reg, key, modifiers, decorators);
                let key_idx = c.add_str(key);
                let line = c.line;
                if modifiers.is_static {
                    c.chunk.emit(OpCode::DefineStatic, line);
                } else {
                    c.chunk.emit(OpCode::Method, line);
                }
                c.chunk
                    .write(Chunk::pack(class_reg, method_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Getter {
                key,
                body: Some(body),
                modifiers,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    &[],
                    body,
                    false,
                    false,
                    !modifiers.is_static,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str(key);
                let line = c.line;
                let op = if modifiers.is_static {
                    OpCode::DefineStaticGetter
                } else {
                    OpCode::DefineGetter
                };
                c.chunk.emit(op, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Setter {
                key,
                param,
                body: Some(body),
                modifiers,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    std::slice::from_ref(param),
                    body,
                    false,
                    false,
                    !modifiers.is_static,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str(key);
                let line = c.line;
                let op = if modifiers.is_static {
                    OpCode::DefineStaticSetter
                } else {
                    OpCode::DefineSetter
                };
                c.chunk.emit(op, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::StaticBlock { body, .. } => {
                let (proto, upvalues) = compile_function(
                    c,
                    Rc::from("<static_block>"),
                    &[],
                    body,
                    false,
                    false,
                    false,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let result = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(result, fn_reg as u8), line);
                c.chunk.write(Chunk::pack(0, 0), line);
                c.free_reg();
                c.free_reg();
            }
            ClassMember::Destructor { body, .. } => {
                let (proto, upvalues) =
                    compile_function(c, Rc::from("dispose"), &[], body, false, false, true);
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str("dispose");
                let line = c.line;
                c.chunk.emit(OpCode::Method, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            _ => {}
        }
    }

    if !has_constructor && !c.pending_field_inits.is_empty() {
        let empty = Stmt::new_with_range(
            varn_core::SourceRange::default(),
            StmtKind::Block { stmts: vec![] },
        );
        let (proto, upvalues) =
            compile_function(c, Rc::from("constructor"), &[], &empty, false, false, true);
        let ctor_reg = emit_closure(c, proto, upvalues, 0);
        let key_idx = c.add_str("constructor");
        let line = c.line;
        c.chunk.emit(OpCode::Method, line);
        c.chunk.write(Chunk::pack(class_reg, ctor_reg as u8), line);
        c.chunk.write(key_idx, line);
        c.free_reg();
    }

    c.current_class = saved_class;
    c.current_superclass = saved_superclass;
    c.pending_field_inits = saved_inits;

    apply_class_decorators(c, class_reg, &decl.decorators);

    class_reg
}

pub fn compile_class_decl<'a>(c: &mut Compiler<'a>, decl: &ClassDecl) {
    let class_reg = compile_class_expr(c, decl);
    if let Some(id) = &decl.id {
        if c.is_global {
            let idx = c.add_str(id);
            let line = c.line;
            c.chunk
                .emit_rrc(OpCode::DefineGlobal, 0, class_reg, idx, line);
        } else {
            c.define_local(id.clone(), class_reg);

            return;
        }
    }
    c.free_reg();
}

pub fn compile_enum_decl<'a>(c: &mut Compiler<'a>, en: &EnumDecl) {
    let enum_reg = compile_enum_expr(c, en);
    let name_idx = c.add_str(&en.id);
    if c.is_global {
        let line = c.line;
        c.chunk
            .emit_rrc(OpCode::DefineGlobal, 0, enum_reg, name_idx, line);
        c.free_reg();
    } else {
        c.define_local(en.id.clone(), enum_reg);
    }
}

fn compile_enum_expr<'a>(c: &mut Compiler<'a>, en: &EnumDecl) -> u8 {
    let name_idx = c.add_str(&en.id);
    let class_reg = c.alloc_reg();
    let line = c.line;
    c.chunk
        .emit_rrc(OpCode::MakeClass, class_reg, 0, name_idx, line);

    let local_name: Rc<str> = Rc::from(format!("__class_{}__", en.id));
    c.define_local(local_name, class_reg);

    let saved_class = c.current_class.clone();
    let saved_superclass = c.current_superclass.clone();
    let saved_inits = std::mem::take(&mut c.pending_field_inits);
    c.current_class = Some(en.id.clone());

    let mut current_tag = 0i64;
    for member in en.members.iter() {
        if let Some(init_expr) = &member.init {
            if let varn_core::ast::ExprKind::IntLiteral { value, .. } = &init_expr.kind {
                current_tag = *value;
            }
        }

        let fields_str = member
            .payload_fields
            .iter()
            .map(|f| f.name.as_ref())
            .collect::<Vec<&str>>()
            .join(",");
        let variant_meta = if fields_str.is_empty() {
            format!("{}.{}", en.id, member.id)
        } else {
            format!("{}.{}:{}", en.id, member.id, fields_str)
        };
        let meta_idx = c.add_str(&variant_meta);

        let tag_reg = c.alloc_reg();
        c.emit_load_int(tag_reg, current_tag);

        let variant_reg = c.alloc_reg();
        let line = c.line;
        c.chunk.emit(OpCode::MakeEnumVariant, line);
        c.chunk.write(Chunk::pack(variant_reg, tag_reg), line);
        c.chunk.write(meta_idx, line);

        c.free_reg();
        current_tag += 1;

        let key_idx = c.add_str(&member.id);
        c.chunk.emit(OpCode::DefineStatic, line);
        c.chunk.write(Chunk::pack(class_reg, variant_reg), line);
        c.chunk.write(key_idx, line);

        c.free_reg();
    }

    for member in &en.body {
        if let ClassMember::Property {
            key,
            init,
            modifiers,
            ..
        } = member
        {
            if modifiers.is_static {
                let val_reg: u8 = if let Some(expr) = init {
                    compile_expr(c, expr) as u8
                } else {
                    let r = c.alloc_reg();
                    c.emit_rr(OpCode::LoadNull, r, 0);
                    r
                };
                let key_idx = c.add_str(key);
                let line = c.line;
                c.chunk.emit(OpCode::DefineStatic, line);
                c.chunk.write(Chunk::pack(class_reg, val_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            } else {
                let key_idx = c.add_str(key);
                let line = c.line;
                c.chunk.emit(OpCode::DeclareField, line);
                c.chunk.write(Chunk::pack(class_reg, 0), line);
                c.chunk.write(key_idx, line);
                if let Some(expr) = init {
                    c.pending_field_inits.push((key.clone(), expr.clone()));
                }
            }
        }
    }

    let mut has_constructor = false;
    for member in &en.body {
        match member {
            ClassMember::Constructor { params, body, .. } => {
                has_constructor = true;
                let (proto, upvalues) =
                    compile_function(c, "constructor".into(), params, body, false, false, true);
                let ctor_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str("constructor");
                let line = c.line;
                c.chunk.emit(OpCode::Method, line);
                c.chunk.write(Chunk::pack(class_reg, ctor_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Method {
                key,
                params,
                body: Some(body),
                modifiers,
                decorators,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    params,
                    body,
                    modifiers.is_async,
                    modifiers.is_generator,
                    !modifiers.is_static,
                );
                let method_reg = emit_closure(c, proto, upvalues, 0);
                apply_method_decorators(c, class_reg, method_reg, key, modifiers, decorators);
                let key_idx = c.add_str(key);
                let line = c.line;
                if modifiers.is_static {
                    c.chunk.emit(OpCode::DefineStatic, line);
                } else {
                    c.chunk.emit(OpCode::Method, line);
                }
                c.chunk
                    .write(Chunk::pack(class_reg, method_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Getter {
                key,
                body: Some(body),
                modifiers,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    &[],
                    body,
                    false,
                    false,
                    !modifiers.is_static,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str(key);
                let line = c.line;
                let op = if modifiers.is_static {
                    OpCode::DefineStaticGetter
                } else {
                    OpCode::DefineGetter
                };
                c.chunk.emit(op, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::Setter {
                key,
                param,
                body: Some(body),
                modifiers,
                ..
            } => {
                let (proto, upvalues) = compile_function(
                    c,
                    key.clone(),
                    std::slice::from_ref(param),
                    body,
                    false,
                    false,
                    !modifiers.is_static,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str(key);
                let line = c.line;
                let op = if modifiers.is_static {
                    OpCode::DefineStaticSetter
                } else {
                    OpCode::DefineSetter
                };
                c.chunk.emit(op, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            ClassMember::StaticBlock { body, .. } => {
                let (proto, upvalues) = compile_function(
                    c,
                    Rc::from("<static_block>"),
                    &[],
                    body,
                    false,
                    false,
                    false,
                );
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let result = c.alloc_reg();
                let line = c.line;
                c.chunk.emit(OpCode::Call, line);
                c.chunk.write(Chunk::pack(result, fn_reg as u8), line);
                c.chunk.write(Chunk::pack(0, 0), line);
                c.free_reg();
                c.free_reg();
            }
            ClassMember::Destructor { body, .. } => {
                let (proto, upvalues) =
                    compile_function(c, Rc::from("dispose"), &[], body, false, false, true);
                let fn_reg = emit_closure(c, proto, upvalues, 0);
                let key_idx = c.add_str("dispose");
                let line = c.line;
                c.chunk.emit(OpCode::Method, line);
                c.chunk.write(Chunk::pack(class_reg, fn_reg as u8), line);
                c.chunk.write(key_idx, line);
                c.free_reg();
            }
            _ => {}
        }
    }

    if !has_constructor && !c.pending_field_inits.is_empty() {
        let empty = Stmt::new_with_range(
            varn_core::SourceRange::default(),
            StmtKind::Block { stmts: vec![] },
        );
        let (proto, upvalues) =
            compile_function(c, Rc::from("constructor"), &[], &empty, false, false, true);
        let ctor_reg = emit_closure(c, proto, upvalues, 0);
        let key_idx = c.add_str("constructor");
        let line = c.line;
        c.chunk.emit(OpCode::Method, line);
        c.chunk.write(Chunk::pack(class_reg, ctor_reg as u8), line);
        c.chunk.write(key_idx, line);
        c.free_reg();
    }

    let ctor_key = c.add_str("constructor");
    for member in &en.members {
        let mut const_args = vec![];
        for f in &member.payload_fields {
            match &f.ty.kind {
                varn_core::TypeKind::LiteralInt(val) => {
                    const_args.push(varn_core::ast::ExprKind::IntLiteral {
                        value: *val,
                        raw: Rc::from(val.to_string()),
                    });
                }
                varn_core::TypeKind::LiteralStr(val) => {
                    const_args.push(varn_core::ast::ExprKind::StrLiteral { value: val.clone() });
                }
                varn_core::TypeKind::LiteralFloat(bits) => {
                    const_args.push(varn_core::ast::ExprKind::FloatLiteral {
                        value: f64::from_bits(*bits),
                        raw: Rc::from(f64::from_bits(*bits).to_string()),
                    });
                }
                varn_core::TypeKind::LiteralBool(val) => {
                    const_args.push(varn_core::ast::ExprKind::BoolLiteral { value: *val });
                }
                _ => {}
            }
        }

        if !const_args.is_empty() {
            let saved_regs = c.regs.save();

            let var_name_idx = c.add_str(&member.id);
            let receiver_reg = c.alloc_reg();
            c.emit_property(OpCode::GetProperty, receiver_reg, class_reg, var_name_idx);

            let mut arg_count = 1;
            for arg_kind in const_args {
                let arg_expr = varn_core::ast::Expr {
                    id: 0,
                    range: varn_core::SourceRange::default(),
                    kind: arg_kind,
                };
                compile_expr(c, &arg_expr);
                arg_count += 1;
            }

            let ctor_reg = c.alloc_reg();
            c.emit_property(OpCode::GetProperty, ctor_reg, receiver_reg, ctor_key);

            let dest = c.alloc_reg();
            let line = c.line;
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(dest, ctor_reg), line);
            c.chunk
                .write(Chunk::pack(arg_count as u8, receiver_reg), line);

            c.regs.restore(saved_regs);
        }
    }

    c.current_class = saved_class;
    c.current_superclass = saved_superclass;
    c.pending_field_inits = saved_inits;

    class_reg
}

pub fn compile_namespace_decl<'a>(c: &mut Compiler<'a>, ns: &NamespaceDecl) {
    let ns_reg = c.alloc_reg();
    c.define_local(ns.id.clone(), ns_reg);

    c.push_scope();
    let mut members: Vec<(Rc<str>, u8)> = Vec::new();

    for stmt in &ns.body {
        super::stmt::compile_decl(c, stmt);
        // unwrap export wrapper transparently
        let inner = match stmt {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };
        match inner {
            Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let r = c.alloc_reg();
                        if !c.emit_load_var(name, r) {
                            let idx = c.add_str(name);
                            c.emit_rc(OpCode::LoadGlobal, r, idx);
                        }
                        members.push((name.clone(), r));
                    }
                }
            }
            Decl::Function(f) => {
                let r = c.alloc_reg();
                if !c.emit_load_var(&f.id, r) {
                    let idx = c.add_str(&f.id);
                    c.emit_rc(OpCode::LoadGlobal, r, idx);
                }
                members.push((f.id.clone(), r));
            }
            Decl::Class(cl) => {
                if let Some(name) = &cl.id {
                    let r = c.alloc_reg();
                    if !c.emit_load_var(name, r) {
                        let idx = c.add_str(name);
                        c.emit_rc(OpCode::LoadGlobal, r, idx);
                    }
                    members.push((name.clone(), r));
                }
            }
            Decl::Namespace(sub_ns) => {
                let r = c.alloc_reg();
                if !c.emit_load_var(&sub_ns.id, r) {
                    let idx = c.add_str(&sub_ns.id);
                    c.emit_rc(OpCode::LoadGlobal, r, idx);
                }
                members.push((sub_ns.id.clone(), r));
            }
            Decl::Enum(en) => {
                let r = c.alloc_reg();
                if !c.emit_load_var(&en.id, r) {
                    let idx = c.add_str(&en.id);
                    c.emit_rc(OpCode::LoadGlobal, r, idx);
                }
                members.push((en.id.clone(), r));
            }
            Decl::SumType(st) => {
                let r = c.alloc_reg();
                if !c.emit_load_var(&st.id, r) {
                    let idx = c.add_str(&st.id);
                    c.emit_rc(OpCode::LoadGlobal, r, idx);
                }
                members.push((st.id.clone(), r));
            }
            _ => {}
        }
    }

    let count = members.len() as u8;
    let line = c.line;
    c.chunk.emit(OpCode::BuildObject, line);
    c.chunk.write(Chunk::pack(ns_reg, count), line);
    for (name, reg) in &members {
        let key_idx = c.add_str(name);
        c.chunk.write(key_idx, line);
        c.chunk.write(Chunk::pack(*reg, 0), line);
    }
    for _ in &members {
        c.free_reg();
    }

    c.pop_scope();

    if c.is_global {
        let name_idx = c.add_str(&ns.id);
        c.chunk
            .emit_rrc(OpCode::DefineGlobal, 0, ns_reg, name_idx, line);
    }
}

pub fn compile_extension_decl<'a>(c: &mut Compiler<'a>, decl: &ExtensionDecl) {
    use varn_core::{IntrinsicType, TypeKind, TypeTag};
    let type_name = match &decl.target.kind {
        TypeKind::Intrinsic(TypeTag::Int) => IntrinsicType::Int.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Float) => IntrinsicType::Float.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Str) => IntrinsicType::Str.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Bool) => IntrinsicType::Bool.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Char) => IntrinsicType::Char.as_str().to_owned(),
        TypeKind::Named(n, _) => n.clone(),
        TypeKind::Generic(n, _, _) => n.clone(),
        TypeKind::Intrinsic(TypeTag::Array) => IntrinsicType::Array.as_str().to_owned(),
        _ => IntrinsicType::Dynamic.as_str().to_owned(),
    };
    for member in &decl.members {
        match member {
            ExtensionMember::Method(method) => {
                let mangled: Rc<str> = Rc::from(format!("__ext_{}_{}", type_name, method.id));
                let (proto, upvalues) = compile_function(
                    c,
                    mangled.clone(),
                    &method.params,
                    &method.body,
                    method.modifiers.is_async,
                    method.modifiers.is_generator,
                    true,
                );
                let r = emit_closure(c, proto, upvalues, 0);
                let idx = c.add_str(&mangled);
                let line = c.line;
                c.chunk.emit_rrc(OpCode::DefineGlobal, 0, r, idx, line);
                c.free_reg();
            }
            ExtensionMember::Getter { key, body, .. } => {
                let mangled: Rc<str> = Rc::from(format!("__extget_{type_name}_{key}"));
                let (proto, upvalues) =
                    compile_function(c, mangled.clone(), &[], body, false, false, true);
                let r = emit_closure(c, proto, upvalues, 0);
                let idx = c.add_str(&mangled);
                let line = c.line;
                c.chunk.emit_rrc(OpCode::DefineGlobal, 0, r, idx, line);
                c.free_reg();
            }
            ExtensionMember::Setter {
                key, param, body, ..
            } => {
                let mangled: Rc<str> = Rc::from(format!("__extset_{type_name}_{key}"));
                let (proto, upvalues) = compile_function(
                    c,
                    mangled.clone(),
                    std::slice::from_ref(param),
                    body,
                    false,
                    false,
                    true,
                );
                let r = emit_closure(c, proto, upvalues, 0);
                let idx = c.add_str(&mangled);
                let line = c.line;
                c.chunk.emit_rrc(OpCode::DefineGlobal, 0, r, idx, line);
                c.free_reg();
            }
        }
    }
}

pub fn compile_sum_type<'a>(c: &mut Compiler<'a>, st: &SumTypeDecl) {
    let ns_reg = c.alloc_reg();

    let mut variant_regs: Vec<(Rc<str>, u8)> = Vec::new();

    for (tag, variant) in st.variants.iter().enumerate() {
        let var_reg = c.alloc_reg();
        let tag_r = c.alloc_reg();
        c.emit_load_int(tag_r, tag as i64);

        let fields_str = variant
            .fields
            .iter()
            .map(|f| f.name.as_ref())
            .collect::<Vec<&str>>()
            .join(",");
        let variant_meta = if fields_str.is_empty() {
            format!("{}.{}", st.id, variant.name)
        } else {
            format!("{}.{}:{}", st.id, variant.name, fields_str)
        };
        let name_idx = c.add_str(&variant_meta);
        let line = c.line;
        c.chunk.emit(OpCode::MakeEnumVariant, line);
        c.chunk.write(Chunk::pack(var_reg, tag_r), line);
        c.chunk.write(name_idx, line);
        c.free_reg(); // free tag_r

        if c.is_global {
            let name_idx = c.add_str(&variant.name);
            c.chunk
                .emit_rrc(OpCode::DefineGlobal, 0, var_reg, name_idx, line);
        } else {
            c.define_local(variant.name.clone(), var_reg);
        }

        variant_regs.push((variant.name.clone(), var_reg));
    }

    let count = variant_regs.len() as u8;
    let line = c.line;
    c.chunk.emit(OpCode::BuildObject, line);
    c.chunk.write(Chunk::pack(ns_reg, count), line);
    for (name, reg) in &variant_regs {
        let key_idx = c.add_str(name);
        c.chunk.write(key_idx, line);
        c.chunk.write(Chunk::pack(*reg, 0), line);
    }

    if c.is_global {
        let name_idx = c.add_str(&st.id);
        c.chunk
            .emit_rrc(OpCode::DefineGlobal, 0, ns_reg, name_idx, line);

        for _ in &variant_regs {
            c.free_reg();
        }
        c.free_reg();
    } else {
        c.define_local(st.id.clone(), ns_reg);
    }
}

