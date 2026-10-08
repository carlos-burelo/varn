use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprKind};
use varn_core::AtomInterner;
use varn_modules::layer::Layer;
use varn_tir::{TirImport, TirImportKind, TirImportSpec};

pub(super) fn prelude_imports(
    module: &str,
    declared: &FxHashSet<Arc<str>>,
    ast_arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<TirImport> {
    if Layer::of_module(module) == Layer::Core {
        return Vec::new();
    }
    let used: FxHashSet<&str> = ast_arena
        .exprs()
        .filter_map(|e| match &e.kind {
            ExprKind::Identifier { name } => interner.try_resolve(*name),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => None,
        })
        .collect();
    varn_modules::prelude_modules()
        .into_iter()
        .map(|spec| TirImport {
            source: Arc::from(spec.id),
            is_type_only: false,
            specs: spec
                .exports
                .iter()
                .filter(|name| used.contains(**name) && !declared.contains(**name))
                .map(|name| TirImportSpec {
                    local: Arc::from(*name),
                    kind: TirImportKind::Named(Arc::from(*name)),
                })
                .collect(),
        })
        .filter(|import| !import.specs.is_empty())
        .collect()
}
