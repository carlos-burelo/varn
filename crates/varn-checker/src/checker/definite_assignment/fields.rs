use super::ctor_flow::CtorFlow;
use super::patterns::walk_expr_children;
use crate::checker::Checker;
use rustc_hash::FxHashSet;
use varn_core::ast::{AstTypeKind, ClassDecl, ClassMember, ExprId, ExprKind, TypeNode};
use varn_core::source::SourceRange;
use varn_core::{Atom, Diagnostic, ErrorCode, LangPrimitive, TypeKind};
use varn_sem::bind::BindResult;

fn admits_null(kind: &AstTypeKind) -> bool {
    match kind {
        TypeKind::Primitive(LangPrimitive::Null | LangPrimitive::Dynamic) => true,
        TypeKind::Union(members) => members.iter().any(|m| admits_null(&m.kind)),
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Record(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => false,
    }
}

struct Field {
    name: Atom,
    range: SourceRange,
    init: Option<ExprId>,
    required: bool,
}

fn instance_fields(class: &ClassDecl) -> Vec<Field> {
    class
        .body
        .iter()
        .filter_map(|member| {
            let ClassMember::Property {
                key,
                type_ann,
                init,
                modifiers,
                range,
                ..
            } = member
            else {
                return None;
            };
            if modifiers.is_static || modifiers.is_abstract || modifiers.is_declare {
                return None;
            }
            let ann: Option<&TypeNode> = type_ann.as_ref();
            let required = init.is_none() && ann.is_some_and(|a| !admits_null(&a.kind));
            Some(Field {
                name: *key,
                range: *range,
                init: *init,
                required,
            })
        })
        .collect()
}

impl<'r> Checker<'r> {
    pub(super) fn check_class_fields(&mut self, class: &ClassDecl, bind: &BindResult) {
        if class.modifiers.is_declare || class.modifiers.is_native {
            return;
        }
        let fields = instance_fields(class);
        self.check_initializer_order(&fields, bind);

        let required: FxHashSet<Atom> = fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        if required.is_empty() {
            return;
        }
        let constructors: Vec<_> = class
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::Constructor { body, .. } => Some(*body),
                ClassMember::Destructor { .. }
                | ClassMember::Method { .. }
                | ClassMember::Property { .. }
                | ClassMember::Getter { .. }
                | ClassMember::Setter { .. }
                | ClassMember::StaticBlock { .. } => None,
            })
            .collect();

        let mut unassigned: FxHashSet<Atom> = FxHashSet::default();
        if constructors.is_empty() {
            unassigned.clone_from(&required);
        }
        for body in constructors {
            let mut flow = CtorFlow::new(self.ast_arena, &required);
            flow.run(body);
            unassigned.extend(flow.unassigned_at_exit.iter().copied());
            for (field, range) in flow.early_reads {
                let name = bind.interner.resolve(field);
                self.emit(
                    Diagnostic::error(
                        ErrorCode::UseBeforeAssignment,
                        format!("field '{name}' is read before the constructor assigns it"),
                    )
                    .with_range(range),
                );
            }
        }
        for field in fields.iter().filter(|f| unassigned.contains(&f.name)) {
            let name = bind.interner.resolve(field.name);
            self.emit(
                Diagnostic::error(
                    ErrorCode::UninitializedField,
                    format!(
                        "field '{name}' is not initialized: give it an initializer, make it nullable or assign it on every path of the constructor"
                    ),
                )
                .with_range(field.range),
            );
        }
    }

    fn check_initializer_order(&mut self, fields: &[Field], bind: &BindResult) {
        for (at, field) in fields.iter().enumerate() {
            let Some(init) = field.init else {
                continue;
            };
            let ready: FxHashSet<Atom> = fields[..at]
                .iter()
                .filter(|f| f.init.is_some() || !f.required)
                .map(|f| f.name)
                .collect();
            let declared: FxHashSet<Atom> = fields.iter().map(|f| f.name).collect();
            let mut reads = Vec::new();
            self.collect_this_reads(init, &mut reads);
            for (name, range) in reads {
                let nullable_later = fields
                    .iter()
                    .any(|f| f.name == name && !f.required && f.init.is_none());
                if declared.contains(&name) && !ready.contains(&name) && !nullable_later {
                    let text = bind.interner.resolve(name);
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::UseBeforeAssignment,
                            format!(
                                "field '{text}' is read in an initializer before it is initialized"
                            ),
                        )
                        .with_range(range),
                    );
                }
            }
        }
    }

    fn collect_this_reads(&self, e: ExprId, out: &mut Vec<(Atom, SourceRange)>) {
        let arena = self.ast_arena;
        match &arena.expr(e).kind {
            ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::ClassExpr { .. } => {}
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } if matches!(arena.expr(*object).kind, ExprKind::This) => {
                if let ExprKind::Identifier { name } = &arena.expr(*property).kind {
                    out.push((*name, arena.expr(e).range));
                }
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::ImportMeta(_)
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
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => {
                walk_expr_children(e, arena, &mut |c| self.collect_this_reads(c, out))
            }
        }
    }
}
