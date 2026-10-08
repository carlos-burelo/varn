use crate::varn_contract::{classify, Mapped};
use varn_core::ast::{
    AstArena, ClassDecl, ClassMember, Decl, ExportDecl, ExprKind, FunctionDecl, Param, Pattern,
    StmtId, StmtKind, TypeNode,
};
use varn_core::AtomInterner;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Method,
    Getter,
    StaticMethod,
    StaticGetter,
    Constructor,
    Property,

    Function,
}

pub(crate) struct ParamInfo {
    pub(crate) mapped: Mapped,
    pub(crate) is_rest: bool,
}

pub(crate) struct Member {
    pub(crate) symbol: String,
    pub(crate) kind: Kind,
    pub(crate) params: Vec<ParamInfo>,
    pub(crate) ret: Mapped,

    pub(crate) fallible: bool,
}

pub(crate) fn param_name_is_rest(p: &Param) -> bool {
    p.is_rest || matches!(p.pattern, Pattern::Rest { .. })
}

pub(crate) fn is_fallible(
    decorators: &[varn_core::ast::Decorator],
    arena: &AstArena,
    interner: &AtomInterner,
) -> bool {
    decorators.iter().any(|d| {
        matches!(&arena.expr(d.expression).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "fallible")
    })
}

pub(crate) fn collect_members(
    class_name: &str,
    decl: &ClassDecl,
    arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<Member> {
    let mut out = Vec::new();
    for m in &decl.body {
        match m {
            ClassMember::Method {
                key,
                params,
                return_type,
                modifiers,
                decorators,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticMethod
                } else {
                    Kind::Method
                };
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind,
                    params: map_params(params, interner),
                    ret: return_type
                        .as_ref()
                        .map(|t| classify(t, interner))
                        .unwrap_or(Mapped::Void),
                    fallible: is_fallible(decorators, arena, interner),
                });
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticGetter
                } else {
                    Kind::Getter
                };
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind,
                    params: vec![],
                    ret: return_type
                        .as_ref()
                        .map(|t| classify(t, interner))
                        .unwrap_or(Mapped::Dynamic),
                    fallible: false,
                });
            }

            ClassMember::Property {
                key,
                type_ann,
                init: None,
                modifiers,
                ..
            } => {
                let kind = if modifiers.is_static {
                    Kind::StaticGetter
                } else if modifiers.is_readonly {
                    Kind::Getter
                } else {
                    Kind::Property
                };
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind,
                    params: vec![],
                    ret: type_ann
                        .as_ref()
                        .map(|t| classify(t, interner))
                        .unwrap_or(Mapped::Dynamic),
                    fallible: false,
                });
            }
            ClassMember::Constructor { params, .. } => {
                out.push(Member {
                    symbol: "constructor".to_string(),
                    kind: Kind::Constructor,
                    params: map_params(params, interner),
                    ret: Mapped::Dynamic,
                    fallible: false,
                });
            }
            ClassMember::Destructor { .. }
            | ClassMember::Property { .. }
            | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => {}
        }
    }
    let _ = class_name;
    out
}

pub(crate) fn collect_functions(
    body: &[StmtId],
    arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<Member> {
    fn from_decl(decl: &Decl, interner: &AtomInterner, out: &mut Vec<Member>) {
        match decl {
            Decl::Function(f) => out.push(function_member(f, interner)),
            Decl::Export(ExportDecl::Decl { declaration, .. }) => {
                from_decl(declaration, interner, out)
            }
            Decl::Variable(_)
            | Decl::Class(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Namespace(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }
    let mut out = Vec::new();
    for &stmt_id in body {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt_id).kind {
            from_decl(decl, interner, &mut out);
        }
    }
    out
}

pub(crate) fn function_member(f: &FunctionDecl, interner: &AtomInterner) -> Member {
    Member {
        symbol: interner.resolve(f.id).to_string(),
        kind: Kind::Function,
        params: map_params(&f.params, interner),
        ret: f
            .return_type
            .as_ref()
            .map(|t| classify(t, interner))
            .unwrap_or(Mapped::Void),
        fallible: false,
    }
}

pub(crate) fn param_type(p: &Param) -> Option<&TypeNode> {
    p.type_ann.as_ref()
}

pub(crate) fn map_params(params: &[Param], interner: &AtomInterner) -> Vec<ParamInfo> {
    params
        .iter()
        .map(|p| {
            let is_rest = param_name_is_rest(p);
            let base = param_type(p)
                .map(|t| classify(t, interner))
                .unwrap_or(Mapped::Dynamic);
            let mapped = if p.is_optional && !is_rest {
                Mapped::Opt(Box::new(base))
            } else {
                base
            };
            ParamInfo { mapped, is_rest }
        })
        .collect()
}

pub(crate) fn find_class(
    body: &[StmtId],
    arena: &AstArena,
    name: &str,
    interner: &AtomInterner,
) -> Option<ClassDecl> {
    for &stmt_id in body {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt_id).kind {
            if let Some(c) = class_from_decl(decl, name, interner) {
                return Some(c);
            }
        }
    }
    None
}

pub(crate) fn class_from_decl(
    decl: &Decl,
    name: &str,
    interner: &AtomInterner,
) -> Option<ClassDecl> {
    match decl {
        Decl::Class(c) => {
            if c.id.map(|id| interner.resolve(id)) == Some(name) {
                Some(c.clone())
            } else {
                None
            }
        }
        Decl::Export(ExportDecl::Decl { declaration, .. }) => {
            class_from_decl(declaration, name, interner)
        }
        Decl::Variable(_)
        | Decl::Function(_)
        | Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Enum(_)
        | Decl::Namespace(_)
        | Decl::Import(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => None,
    }
}
