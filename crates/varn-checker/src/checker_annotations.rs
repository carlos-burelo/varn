use crate::binder::infer_expr_type;
use crate::binder::resolve_type_node;
use crate::symbol::SymbolKind;
use crate::types::{Type, TypeContext};
use crate::BindResult;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{Arg, ArrayEl, Decl, Expr, ExprKind, ForInit, Program, Stmt, StmtKind};
use varn_core::ast::{ExportDecl, ImportSpecifier};
use varn_core::TypeKind;
use varn_core::{intrinsic_ops::intrinsic_lookup, NumericKind, TypeAnnotations};

#[derive(Clone)]
struct AnnotateCtx<'a> {
    bind: &'a BindResult,
    locals: rustc_hash::FxHashMap<std::rc::Rc<str>, Type>,
    resolved_expr_types: &'a rustc_hash::FxHashMap<u32, Type>,
}

impl<'a> AnnotateCtx<'a> {
    fn new(
        bind: &'a BindResult,
        resolved_expr_types: &'a rustc_hash::FxHashMap<u32, Type>,
    ) -> Self {
        Self {
            bind,
            locals: rustc_hash::FxHashMap::default(),
            resolved_expr_types,
        }
    }
}

impl<'a> TypeContext for AnnotateCtx<'a> {
    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.bind.get_interface_members(name, origin)
    }

    fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.bind.get_class_members(name, origin)
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.bind.get_namespace_members(name, origin)
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(ty) = self.locals.get(name) {
            return Some(ty.clone());
        }
        self.bind.resolve_symbol(name)
    }

    fn source_file(&self) -> Option<&str> {
        self.bind.source_file()
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, varn_core::ast::TypeNode)> {
        self.bind.get_alias_node(name)
    }
}

fn extract_caps_from_decorators(decorators: &[varn_core::ast::Decorator]) -> Vec<String> {
    let mut caps = Vec::new();
    for dec in decorators {
        if let varn_core::ast::ExprKind::Call { callee, args, .. } = &dec.expression.kind {
            let is_cap_fn = matches!(
                &callee.kind,
                varn_core::ast::ExprKind::Identifier { name } if name.as_ref() == "cap"
            );
            if !is_cap_fn {
                continue;
            }
            if let Some(first_arg) = args.first() {
                let value_expr = match first_arg {
                    varn_core::ast::Arg::Positional(e) => e,
                    varn_core::ast::Arg::Named { value, .. } => value,
                    varn_core::ast::Arg::Spread(e) => e,
                };
                if let varn_core::ast::ExprKind::StrLiteral { value } = &value_expr.kind {
                    caps.push(value.clone());
                }
            }
        }
    }
    caps
}

fn get_expr_type(expr: &Expr, ctx: &AnnotateCtx) -> Type {
    if let Some(ty) = ctx.resolved_expr_types.get(&expr.id) {
        ty.clone()
    } else {
        infer_expr_type(expr, Some(ctx))
    }
}

/// The native core class a receiver type belongs to, if any. Only core types
/// whose methods are natively registered (and thus op-id-addressable).
fn core_class_of_type(ty: &Type) -> Option<&'static str> {
    match &ty.0 {
        // `Array` types are structural (`T[]`), not `Named`, so they bypass the
        // name lookup — resolve the class name from the canonical core table.
        TypeKind::Array(_) => varn_core::op_id::core_class_name(varn_core::TypeTag::Array),
        TypeKind::Named(name, _) => varn_core::op_id::core_class(name.as_ref()),
        _ => None,
    }
}

/// Whether `method` is a member of core class `class` (per the loaded `.vn`
/// contracts). Guards op-id emission so we never emit a `CallNativeOp` whose id
/// would not resolve at runtime.
fn core_has_method(bind: &BindResult, class: &str, method: &str) -> bool {
    bind.core
        .as_ref()
        .and_then(|c| c.class_members.get(class))
        .map_or(false, |info| {
            info.members.iter().any(|m| {
                m.name.as_ref() == method
                    // Only instance methods: static methods (`str.from`,
                    // `Array.isArray`) take no receiver, so the op-id arg layout
                    // (receiver at args[0]) would not match their wrapper.
                    // Getters/setters/ctors aren't valid in call position; async/
                    // generators need promise/iterator handling.
                    && matches!(m.kind, crate::types::ClassMemberKind::Method)
                    && !m.is_static
                    && !m.is_async
                    && !m.is_generator
            })
        })
}

pub fn collect_type_annotations(
    program: &Program,
    bind: &BindResult,
    resolved_expr_types: &rustc_hash::FxHashMap<u32, Type>,
) -> TypeAnnotations {
    let mut ann = TypeAnnotations::new();
    let mut ctx = AnnotateCtx::new(bind, resolved_expr_types);
    for stmt in &program.body {
        annotate_stmt(stmt, &mut ann, &mut ctx);
    }

    for stmt in &program.body {
        if let StmtKind::Decl(decl_node) = &stmt.kind {
            if let Decl::Export(ExportDecl::Decl { declaration, .. }) = &**decl_node {
                if let Decl::Function(f) = declaration.as_ref() {
                    for cap in extract_caps_from_decorators(&f.decorators) {
                        ann.record_module_cap(cap);
                    }
                }
            }
        }
    }
    ann
}

fn annotate_stmt(stmt: &Stmt, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match &stmt.kind {
        StmtKind::Expr { expression } => annotate_expr(expression, ann, ctx),
        StmtKind::Return {
            argument: Some(arg),
        } => annotate_expr(arg, ann, ctx),
        StmtKind::Return { .. } => {}
        StmtKind::Block { stmts } => {
            let old_locals = ctx.locals.clone();
            for s in stmts {
                annotate_stmt(s, ann, ctx);
            }
            ctx.locals = old_locals;
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            annotate_expr(test, ann, ctx);
            annotate_stmt(consequent, ann, ctx);
            if let Some(alt) = alternate {
                annotate_stmt(alt, ann, ctx);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
            annotate_expr(test, ann, ctx);
            annotate_stmt(body, ann, ctx);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            let old_locals = ctx.locals.clone();
            if let Some(init_box) = init {
                match init_box.as_ref() {
                    ForInit::Expr(e) => annotate_expr(e, ann, ctx),
                    ForInit::Var { declarators, .. } => {
                        for d in declarators {
                            if let Some(init_expr) = &d.init {
                                annotate_expr(init_expr, ann, ctx);
                            }
                            let name_opt = match &d.id {
                                varn_core::ast::Pattern::Identifier { name, .. } => {
                                    Some(name.clone())
                                }
                                _ => None,
                            };
                            if let Some(name) = name_opt {
                                let type_ann = d.type_ann.as_ref().or(match &d.id {
                                    varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                        type_ann.as_ref()
                                    }
                                    _ => None,
                                });
                                if let Some(ann_node) = type_ann {
                                    let ty = resolve_type_node(ann_node, Some(ctx.bind));
                                    ctx.locals.insert(name, ty);
                                } else if let Some(init_expr) = &d.init {
                                    let ty = get_expr_type(init_expr, ctx);
                                    ctx.locals.insert(name, ty);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(t) = test {
                annotate_expr(t, ann, ctx);
            }
            if let Some(u) = update {
                annotate_expr(u, ann, ctx);
            }
            annotate_stmt(body, ann, ctx);
            ctx.locals = old_locals;
        }
        StmtKind::Decl(decl) => annotate_decl(decl, ann, ctx),
        _ => {}
    }
}

fn annotate_decl(decl: &Decl, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match decl {
        Decl::Variable(v) => {
            for d in &v.declarators {
                if let Some(init) = &d.init {
                    annotate_expr(init, ann, ctx);
                }
                let name_opt = match &d.id {
                    varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = name_opt {
                    let type_ann = d.type_ann.as_ref().or(match &d.id {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    if let Some(ann_node) = type_ann {
                        let ty = resolve_type_node(ann_node, Some(ctx.bind));
                        ctx.locals.insert(name, ty);
                    } else if let Some(init) = &d.init {
                        let ty = get_expr_type(init, ctx);
                        ctx.locals.insert(name, ty);
                    }
                }
            }
        }
        Decl::Function(f) => {
            if !f.modifiers.is_declare {
                let mut local_ctx = ctx.clone();
                for p in &f.params {
                    let name_opt = match &p.pattern {
                        varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
                        _ => None,
                    };
                    if let Some(name) = name_opt {
                        let type_ann = p.type_ann.as_ref().or(match &p.pattern {
                            varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                type_ann.as_ref()
                            }
                            _ => None,
                        });
                        let ty = if let Some(ann_node) = type_ann {
                            resolve_type_node(ann_node, Some(ctx.bind))
                        } else {
                            Type::Dynamic
                        };
                        local_ctx.locals.insert(name, ty);
                    }
                }
                annotate_stmt(&f.body, ann, &mut local_ctx);
            }
        }
        Decl::Import(i) => {
            for spec in &i.specifiers {
                let name = match spec {
                    ImportSpecifier::Default { local, .. } => local,
                    ImportSpecifier::Named { local, .. } => local,
                    ImportSpecifier::Namespace { local, .. } => local,
                };
                let scope = ctx.bind.scopes.get(ctx.bind.global_scope);
                if let Some(id) = scope.resolve(name, &ctx.bind.scopes) {
                    let sym = ctx.bind.arena.get(id);
                    if let Some(slot_idx) = sym.slot_idx {
                        ann.record_slot_idx(spec.range().start.offset, slot_idx);
                    }
                    if matches!(
                        sym.kind,
                        SymbolKind::Interface
                            | SymbolKind::TypeAlias
                            | SymbolKind::Struct
                            | SymbolKind::Extension
                    ) {
                        ann.record_type_only(spec.range().start.offset);
                    }
                }
            }
        }
        Decl::Export(e) => {
            let exports = if ctx.bind.source_file.starts_with("std:")
                || ctx.bind.source_file.starts_with("core:")
            {
                crate::module_resolver::resolve_stdlib_module_exports_ref(&ctx.bind.source_file)
            } else {
                crate::module_resolver::resolve_module_exports_ref(
                    &ctx.bind.source_file,
                    &mut vec![],
                )
            };
            match e {
                ExportDecl::Decl { declaration, .. } => {
                    annotate_decl(declaration, ann, ctx);
                    if let Some(name) = decl_primary_name(declaration) {
                        if let Some(sym) = exports.get(name.as_ref()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(declaration.range().start.offset, slot_idx);
                            }
                        }
                    }
                }
                ExportDecl::Default {
                    declaration, range, ..
                } => {
                    match declaration.as_ref() {
                        varn_core::ast::ExportDefaultDecl::Function(f) => {
                            annotate_decl(&Decl::Function(f.clone()), ann, ctx);
                        }
                        varn_core::ast::ExportDefaultDecl::Class(c) => {
                            annotate_decl(&Decl::Class(c.clone()), ann, ctx);
                        }
                        varn_core::ast::ExportDefaultDecl::Expr(ex) => {
                            annotate_expr(ex, ann, ctx);
                        }
                    }
                    if let Some(sym) = exports.get("default") {
                        if let Some(slot_idx) = sym.slot_idx {
                            ann.record_slot_idx(range.start.offset, slot_idx);
                        }
                    }
                }
                ExportDecl::Named { specifiers, .. } => {
                    for spec in specifiers {
                        if let Some(sym) = exports.get(&spec.exported.to_string()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(spec.range.start.offset, slot_idx);
                            }
                        }
                    }
                }
                ExportDecl::All { alias, range, .. } => {
                    if let Some(ns) = alias {
                        if let Some(sym) = exports.get(&ns.to_string()) {
                            if let Some(slot_idx) = sym.slot_idx {
                                ann.record_slot_idx(range.start.offset, slot_idx);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn annotate_expr(expr: &Expr, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match &expr.kind {
        ExprKind::Binary { op, left, right } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);

            let is_arithmetic = matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
            );
            let is_comparison = matches!(
                op,
                BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::Eq
                    | BinaryOp::NotEq
            );
            if !is_arithmetic && !is_comparison {
                return;
            }

            let l = get_expr_type(left, ctx);
            let r = get_expr_type(right, ctx);

            // The numeric annotation records the OPERAND class of the
            // operation (it drives typed-opcode selection). The result
            // class — where `int / int → float` differs — comes from
            // `varn_core::numeric::binary_result_kind` at HIR lowering.
            use varn_core::{binary_operand_kind, NumericOperand, TypeTag};
            let operand = |tk: &TypeKind<_, _, _, _, _, _>| match tk {
                TypeKind::Intrinsic(TypeTag::Int) | TypeKind::LiteralInt(_) => {
                    Some(NumericOperand::Int)
                }
                TypeKind::Intrinsic(TypeTag::Float) | TypeKind::LiteralFloat(_) => {
                    Some(NumericOperand::Float)
                }
                TypeKind::Intrinsic(TypeTag::Decimal) => Some(NumericOperand::Decimal),
                _ => None,
            };

            let kind = match binary_operand_kind(operand(&l.0), operand(&r.0)) {
                Some(NumericOperand::Int) => Some(NumericKind::Int),
                Some(NumericOperand::Float) => Some(NumericKind::Float),
                // Decimal has no typed opcodes; the generic path handles it.
                Some(NumericOperand::Decimal) | None => None,
            };
            if let Some(k) = kind {
                ann.record_numeric(expr.range.start.offset, k);
            }
        }
        ExprKind::Paren { expression } => annotate_expr(expression, ann, ctx),
        ExprKind::Unary { operand, .. } => annotate_expr(operand, ann, ctx),
        ExprKind::Logical { left, right, .. } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);
        }
        ExprKind::Assign { value, target, .. } => {
            annotate_expr(target, ann, ctx);
            annotate_expr(value, ann, ctx);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            annotate_expr(test, ann, ctx);
            annotate_expr(consequent, ann, ctx);
            annotate_expr(alternate, ann, ctx);
        }
        ExprKind::Call { callee, args, .. } => {
            annotate_expr(callee, ann, ctx);
            for arg in args {
                let e = match arg {
                    Arg::Positional(e) => e,
                    Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                annotate_expr(e, ann, ctx);
            }

            if let ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } = &callee.kind
            {
                if let ExprKind::Identifier { name: prop_name } = &property.kind {
                    let obj_ty = get_expr_type(object, ctx);
                    let obj_ty_nn = obj_ty.non_nullified();
                    let mut recorded = false;
                    if let TypeKind::Named(_, Some(ref origin_path)) = &obj_ty_nn.0 {
                        let key = format!("{}/{}", origin_path, prop_name);
                        if let Some(wire_byte) = intrinsic_lookup(&key) {
                            ann.record_intrinsic(expr.range.start.offset, wire_byte);
                            recorded = true;
                        }
                    }
                    // Statically-typed core-type method call (e.g. `arr.push(x)`):
                    // resolve directly to its native op-id so codegen can emit a
                    // direct dispatch, bypassing the string-name + inline-cache
                    // lookup. Only when the receiver is concretely a core class
                    // and the member is one of its native methods.
                    if !recorded {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            if let Some(class) = core_class_of_type(&obj_ty_nn) {
                                if core_has_method(ctx.bind, class, prop_name) {
                                    ann.record_native_op(
                                        expr.range.start.offset,
                                        varn_core::op_id::core_method_op_id(class, prop_name),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            annotate_expr(object, ann, ctx);
            if *computed {
                // Recurse into the index expression
                annotate_expr(property, ann, ctx);
                // Annotate as a typed-array index when the object is statically known to be Array
                let obj_ty = get_expr_type(object, ctx);
                if matches!(obj_ty.non_nullified().0, TypeKind::Array(_)) {
                    ann.record_array_index(expr.range.start.offset);
                }
            } else if let ExprKind::Identifier { name: prop_name } = &property.kind {
                let obj_ty = get_expr_type(object, ctx);
                let check_ty = obj_ty.non_nullified();

                let class_name = match &check_ty.0 {
                    TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => {
                        Some(n.as_ref())
                    }
                    _ => None,
                };
                if let Some(cn) = class_name {
                    if ctx.bind.type_members.classes.contains_key(cn) {
                        let mut hierarchy = Vec::new();
                        let mut current: Option<std::rc::Rc<str>> = Some(std::rc::Rc::from(cn));
                        while let Some(c) = current {
                            hierarchy.push(c.clone());
                            current = ctx.bind.class_parents.get(c.as_ref()).cloned();
                        }
                        hierarchy.reverse();

                        let mut slot = 0u16;
                        let mut known_props: rustc_hash::FxHashSet<std::rc::Rc<str>> =
                            rustc_hash::FxHashSet::default();
                        let mut found = false;

                        for cls in &hierarchy {
                            if let Some(entry) = ctx.bind.get_class_entry(cls) {
                                for m in &entry.members {
                                    if !m.is_static
                                        && (m.kind == crate::types::ClassMemberKind::Property
                                            || m.kind == crate::types::ClassMemberKind::Variable)
                                    {
                                        if known_props.insert(m.name.clone()) {
                                            if m.name.as_ref() == prop_name.as_ref() {
                                                // Key by the property identifier's offset, not the
                                                // member expr's start offset. In a chain `a.b.c`, the
                                                // outer member `(a.b).c` and inner `a.b` share the same
                                                // start offset (`a`), so keying by start collides and the
                                                // inner hop wrongly inherits the outer hop's slot.
                                                ann.record_fixed_field_slot(
                                                    property.range.start.offset,
                                                    slot,
                                                );
                                                found = true;
                                                break;
                                            }
                                            slot += 1;
                                        }
                                    }
                                }
                            }
                            if found {
                                break;
                            }
                        }
                    }
                }
            }
        }

        ExprKind::As { expression, .. } | ExprKind::Satisfies { expression, .. } => {
            annotate_expr(expression, ann, ctx)
        }
        ExprKind::Array { elements } => {
            for el in elements {
                if let ArrayEl::Expr(e) = el {
                    annotate_expr(e, ann, ctx);
                }
            }
        }
        _ => {}
    }
}

fn decl_primary_name(decl: &Decl) -> Option<std::rc::Rc<str>> {
    match decl {
        Decl::Variable(v) => v.declarators.first().and_then(|d| match &d.id {
            varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
            _ => None,
        }),
        Decl::Function(f) => Some(f.id.clone()),
        Decl::Class(c) => c.id.clone(),
        Decl::Enum(e) => Some(e.id.clone()),
        Decl::Interface(i) => Some(i.id.clone()),
        Decl::TypeAlias(t) => Some(t.id.clone()),
        Decl::Namespace(n) => Some(n.id.clone()),
        Decl::Struct(s) => Some(s.id.clone()),
        Decl::SumType(s) => Some(s.id.clone()),
        _ => None,
    }
}
