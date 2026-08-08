pub(crate) mod exprs;
pub(crate) mod stmts;

use crate::types::{Type, TypeContext};
use crate::BindResult;
use stmts::annotate_stmt;
use varn_core::ast::{Decl, ExportDecl, Expr, ExprKind, Program, StmtKind};
use varn_core::TypeAnnotations;

#[derive(Clone)]
pub(crate) struct AnnotateCtx<'a> {
    pub(crate) bind: &'a BindResult,
    pub(crate) locals: rustc_hash::FxHashMap<std::rc::Rc<str>, Type>,
    pub(crate) resolved_expr_types: &'a rustc_hash::FxHashMap<u32, Type>,
    /// Names currently bound to an evolving empty-array local whose element
    /// type the binder proved (Task A0.3'). Their entry in `locals` holds
    /// the advisory `Array<T>`; `resolved_expr_types` deliberately kept them
    /// (and their `x[i]` reads) `Dynamic` for diagnostics, so `get_expr_type`
    /// must recompute governed expressions from `locals` instead. Kept in
    /// lockstep with `locals` (save/restore at block/for boundaries,
    /// cleared when entering a nested closure body).
    pub(crate) evolved_locals: rustc_hash::FxHashSet<std::rc::Rc<str>>,
}

impl<'a> AnnotateCtx<'a> {
    pub(crate) fn new(
        bind: &'a BindResult,
        resolved_expr_types: &'a rustc_hash::FxHashMap<u32, Type>,
    ) -> Self {
        Self {
            bind,
            locals: rustc_hash::FxHashMap::default(),
            resolved_expr_types,
            evolved_locals: rustc_hash::FxHashSet::default(),
        }
    }

    /// Whether `expr`'s value type is (transitively) governed by an evolving
    /// empty-array local (Task A0.3'). The checker recorded a deliberately
    /// `Dynamic` diagnostic type for these expressions in
    /// `resolved_expr_types` (design rule 4), so `get_expr_type` must
    /// recompute the more precise, codegen-only type from the `locals`
    /// overlay via `infer_expr_type`. Restricted to exactly the shapes the
    /// overlay can answer soundly — an evolved-array identifier, an `x[i]`
    /// index read or `x.length` on one, and arithmetic/paren/unary built
    /// from those — so no other expression's checker-derived type is ever
    /// overridden.
    pub(crate) fn is_overlay_governed(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Identifier { name } => self.evolved_locals.contains(name.as_ref()),
            ExprKind::Paren { expression } => self.is_overlay_governed(expression),
            ExprKind::Unary { operand, .. } => self.is_overlay_governed(operand),
            ExprKind::Member {
                object,
                computed: true,
                ..
            } => self.is_overlay_governed(object),
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => {
                matches!(&property.kind, ExprKind::Identifier { name } if name.as_ref() == "length")
                    && self.is_overlay_governed(object)
            }
            ExprKind::Binary { left, right, .. } => {
                self.is_overlay_governed(left) || self.is_overlay_governed(right)
            }
            _ => false,
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
