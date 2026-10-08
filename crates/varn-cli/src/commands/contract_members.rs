use super::contract_classify::{classify_code, opt_code};
use varn_core::ast::{
    AstArena, ClassDecl, ClassMember, Decl, ExportDecl, ExprKind, FunctionDecl, Param, Pattern,
    StmtId, StmtKind,
};
use varn_core::AtomInterner;

use std::collections::BTreeMap;

pub(crate) struct TableMember {
    pub(crate) symbol: String,
    pub(crate) kind: &'static str,
    pub(crate) params: Vec<(String, bool)>,
    pub(crate) ret: String,
    pub(crate) fallible: bool,
}

fn param_is_rest(p: &Param) -> bool {
    p.is_rest || matches!(p.pattern, Pattern::Rest { .. })
}

fn is_fallible(
    decorators: &[varn_core::ast::Decorator],
    arena: &AstArena,
    interner: &AtomInterner,
) -> bool {
    decorators.iter().any(|d| {
        matches!(&arena.expr(d.expression).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "fallible")
    })
}

fn map_params(params: &[Param], interner: &AtomInterner) -> Vec<(String, bool)> {
    params
        .iter()
        .map(|p| {
            let is_rest = param_is_rest(p);
            let base = p
                .type_ann
                .as_ref()
                .map(|t| classify_code(t, interner))
                .unwrap_or_else(|| "dynamic".to_string());
            let code = if p.is_optional && !is_rest {
                opt_code(base)
            } else {
                base
            };
            (code, is_rest)
        })
        .collect()
}

fn collect_members(
    decl: &ClassDecl,
    arena: &AstArena,
    interner: &AtomInterner,
) -> Vec<TableMember> {
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
                out.push(TableMember {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        "static_method"
                    } else {
                        "method"
                    },
                    params: map_params(params, interner),
                    ret: return_type
                        .as_ref()
                        .map(|t| classify_code(t, interner))
                        .unwrap_or_else(|| "void".to_string()),
                    fallible: is_fallible(decorators, arena, interner),
                });
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                ..
            } => {
                out.push(TableMember {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        "static_getter"
                    } else {
                        "getter"
                    },
                    params: Vec::new(),
                    ret: return_type
                        .as_ref()
                        .map(|t| classify_code(t, interner))
                        .unwrap_or_else(|| "dynamic".to_string()),
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
                out.push(TableMember {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        "static_getter"
                    } else if modifiers.is_readonly {
                        "getter"
                    } else {
                        "property"
                    },
                    params: Vec::new(),
                    ret: type_ann
                        .as_ref()
                        .map(|t| classify_code(t, interner))
                        .unwrap_or_else(|| "dynamic".to_string()),
                    fallible: false,
                });
            }
            ClassMember::Constructor { params, .. } => {
                out.push(TableMember {
                    symbol: "constructor".to_string(),
                    kind: "constructor",
                    params: map_params(params, interner),
                    ret: "dynamic".to_string(),
                    fallible: false,
                });
            }
            ClassMember::Destructor { .. }
            | ClassMember::Property { .. }
            | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => {}
        }
    }
    out
}

fn function_member(f: &FunctionDecl, interner: &AtomInterner) -> TableMember {
    TableMember {
        symbol: interner.resolve(f.id).to_string(),
        kind: "function",
        params: map_params(&f.params, interner),
        ret: f
            .return_type
            .as_ref()
            .map(|t| classify_code(t, interner))
            .unwrap_or_else(|| "void".to_string()),
        fallible: false,
    }
}

fn for_each_top_decl(body: &[StmtId], arena: &AstArena, mut f: impl FnMut(&Decl)) {
    for &stmt_id in body {
        if let StmtKind::Decl(decl) = &arena.stmt(stmt_id).kind {
            f(decl);
        }
    }
}

fn unwrap_export(decl: &Decl) -> &Decl {
    match decl {
        Decl::Export(ExportDecl::Decl { declaration, .. }) => unwrap_export(declaration),
        Decl::Variable(_)
        | Decl::Function(_)
        | Decl::Class(_)
        | Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Enum(_)
        | Decl::Namespace(_)
        | Decl::Import(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => decl,
    }
}

pub(crate) struct ParsedFile {
    pub(crate) classes: BTreeMap<String, Vec<TableMember>>,
    pub(crate) functions: Vec<TableMember>,
}

pub(crate) fn parse_contract_file(source: &str, name: &str) -> Result<ParsedFile, String> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, name);
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, name, AtomInterner::new())
            .map_err(|_| format!("failed to parse contract `{name}`"))?;
    let mut classes = BTreeMap::new();
    let mut functions = Vec::new();
    for_each_top_decl(&program.body, &arena, |decl| match unwrap_export(decl) {
        Decl::Class(c) => {
            if let Some(id) = c.id {
                classes.insert(
                    interner.resolve(id).to_string(),
                    collect_members(c, &arena, &interner),
                );
            }
        }
        Decl::Function(f) => functions.push(function_member(f, &interner)),
        Decl::Variable(_)
        | Decl::Interface(_)
        | Decl::TypeAlias(_)
        | Decl::Enum(_)
        | Decl::Namespace(_)
        | Decl::Import(_)
        | Decl::Export(_)
        | Decl::Extension(_)
        | Decl::Struct(_)
        | Decl::SumType(_) => {}
    });
    Ok(ParsedFile { classes, functions })
}
