pub(crate) mod exprs;
pub(crate) mod stmts;

use crate::types::{Type, TypeContext};
use crate::BindResult;
use stmts::annotate_stmt;
use varn_core::ast::{Decl, ExportDecl, Program, StmtKind};
use varn_core::TypeAnnotations;

#[derive(Clone)]
pub(crate) struct AnnotateCtx<'a> {
    pub(crate) bind: &'a BindResult,
    /// Paired with `bind` to answer cross-module questions. Annotation runs
    /// after checking, but it still resolves imported types, so it needs the
    /// same capability the checker had.
    pub(crate) resolver: &'a dyn crate::module_resolver::ImportResolver,
    pub(crate) locals: rustc_hash::FxHashMap<std::rc::Rc<str>, Type>,
    pub(crate) expr_table:
        &'a rustc_hash::FxHashMap<varn_core::ast::AstId, crate::checker::TypeEntry>,
}

impl<'a> AnnotateCtx<'a> {
    pub(crate) fn new(
        bind: &'a BindResult,
        resolver: &'a dyn crate::module_resolver::ImportResolver,
        expr_table: &'a rustc_hash::FxHashMap<varn_core::ast::AstId, crate::checker::TypeEntry>,
    ) -> Self {
        Self {
            bind,
            resolver,
            locals: rustc_hash::FxHashMap::default(),
            expr_table,
        }
    }

    /// `bind` viewed with the capability to follow its imports.
    fn view(&self) -> crate::binder::BindView<'_> {
        crate::binder::BindView::new(self.bind, self.resolver)
    }
}

impl<'a> TypeContext for AnnotateCtx<'a> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.view().get_interface_members(name, origin)
    }

    fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.view().get_class_members(name, origin)
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.view().get_namespace_members(name, origin)
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(ty) = self.locals.get(name) {
            return Some(ty.clone());
        }
        self.view().resolve_symbol(name)
    }

    fn source_file(&self) -> Option<&str> {
        // Straight off the bind: this is the module's own name, not something
        // that has to be resolved, and a view is a temporary.
        Some(self.bind.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, varn_core::ast::TypeNode)> {
        self.view().get_alias_node(name)
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
    resolver: &dyn crate::module_resolver::ImportResolver,
    expr_table: &rustc_hash::FxHashMap<varn_core::ast::AstId, crate::checker::TypeEntry>,
) -> TypeAnnotations {
    let mut ann = TypeAnnotations::new();
    let mut ctx = AnnotateCtx::new(bind, resolver, expr_table);
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
