use std::collections::BTreeMap;
use varn_core::ast::decorator_kind::ContractDecorator;

use varn_core::ast::{
    AstArena, ClassDecl, ClassMember, Decl, ExportDecl, ExprKind, FunctionDecl, Param, Pattern,
    StmtKind, TypeNode,
};
use varn_core::kinds::TypeKind;
use varn_core::{AtomInterner, LangPrimitive};

use crate::contract_members::{Kind, Member, ParamInfo};
use crate::varn_contract::Mapped;

pub(crate) enum LookupFailure {
    Unparsable(String),
    BadDecorator(String),
    ClassNotFound,
    NoFunctions,
}

pub(crate) struct ContractSection {
    pub(crate) members: Vec<Member>,
    pub(crate) extends: Option<String>,
}

struct Contract {
    classes: BTreeMap<String, ContractSection>,
    functions: Vec<Member>,
}

pub(crate) fn lookup(
    source: &str,
    name: &str,
    class: Option<&str>,
) -> Result<ContractSection, LookupFailure> {
    let mut contract = parse(source, name)?;
    match class {
        Some(class) => contract
            .classes
            .remove(class)
            .ok_or(LookupFailure::ClassNotFound),
        None if contract.functions.is_empty() => Err(LookupFailure::NoFunctions),
        None => Ok(ContractSection {
            members: contract.functions,
            extends: None,
        }),
    }
}

fn parse(source: &str, name: &str) -> Result<Contract, LookupFailure> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, name);
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, name, AtomInterner::new())
            .map_err(|_| LookupFailure::Unparsable(format!("cannot parse contract `{name}`")))?;
    let mut classes = BTreeMap::new();
    let mut functions = Vec::new();
    for &stmt in &program.body {
        let StmtKind::Decl(decl) = &arena.stmt(stmt).kind else {
            continue;
        };
        match unwrap_export(decl) {
            Decl::Class(c) => {
                if let Some(id) = c.id {
                    let extends = c.super_class.and_then(|e| {
                        if let ExprKind::Identifier { name } = &arena.expr(e).kind {
                            Some(interner.resolve(*name).to_string())
                        } else {
                            None
                        }
                    });
                    classes.insert(
                        interner.resolve(id).to_string(),
                        ContractSection {
                            members: class_members(c, &arena, &interner)?,
                            extends,
                        },
                    );
                }
            }
            Decl::Function(f) => functions.push(function_member(f, &arena, &interner)?),
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
        }
    }
    Ok(Contract { classes, functions })
}

pub(crate) fn unwrap_export(decl: &Decl) -> &Decl {
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

fn scalar(p: LangPrimitive) -> Mapped {
    match p {
        LangPrimitive::Int => Mapped::Int,
        LangPrimitive::Float => Mapped::Float,
        LangPrimitive::Bool => Mapped::Bool,
        LangPrimitive::Char => Mapped::Char,
        LangPrimitive::Str => Mapped::Str,
        LangPrimitive::Void => Mapped::Void,
        LangPrimitive::Null
        | LangPrimitive::BigInt
        | LangPrimitive::Decimal
        | LangPrimitive::Never
        | LangPrimitive::Dynamic => Mapped::Dynamic,
    }
}

pub(crate) fn classify(t: &TypeNode, interner: &AtomInterner) -> Mapped {
    match &t.kind {
        TypeKind::Named(n, _) => LangPrimitive::from_str(interner.resolve(*n))
            .map(scalar)
            .unwrap_or(Mapped::Dynamic),
        TypeKind::Primitive(LangPrimitive::Void) => Mapped::Void,
        TypeKind::TypePredicate { .. } => Mapped::Bool,
        TypeKind::Array(_) => Mapped::Array,
        TypeKind::Union(members) if members.len() == 2 => {
            let is_null = |m: &TypeNode| matches!(m.kind, TypeKind::Primitive(LangPrimitive::Null));
            if is_null(&members[1]) {
                Mapped::Opt(Box::new(classify(&members[0], interner)))
            } else if is_null(&members[0]) {
                Mapped::Opt(Box::new(classify(&members[1], interner)))
            } else {
                Mapped::Dynamic
            }
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
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
        | TypeKind::EnumVariant { .. } => Mapped::Dynamic,
    }
}

fn classify_or(t: Option<&TypeNode>, interner: &AtomInterner, fallback: Mapped) -> Mapped {
    t.map(|t| classify(t, interner)).unwrap_or(fallback)
}

fn params(params: &[Param], interner: &AtomInterner) -> Vec<ParamInfo> {
    params
        .iter()
        .map(|p| {
            let is_rest = p.is_rest || matches!(p.pattern, Pattern::Rest { .. });
            let base = classify_or(p.type_ann.as_ref(), interner, Mapped::Dynamic);
            let mapped = if p.is_optional && !is_rest {
                Mapped::Opt(Box::new(base))
            } else {
                base
            };
            ParamInfo { mapped, is_rest }
        })
        .collect()
}

fn throws(
    decorators: &[varn_core::ast::Decorator],
    arena: &AstArena,
    interner: &AtomInterner,
) -> Result<Vec<String>, LookupFailure> {
    use varn_core::ast::Arg;
    let bad = || LookupFailure::BadDecorator("`@throws` takes error class names".to_string());
    let mut out = Vec::new();
    for d in decorators {
        let ExprKind::Call { callee, args, .. } = &arena.expr(d.expression).kind else {
            continue;
        };
        let ExprKind::Identifier { name } = &arena.expr(*callee).kind else {
            continue;
        };
        if ContractDecorator::of(interner.resolve(*name)) != Some(ContractDecorator::Throws) {
            continue;
        }
        for arg in args {
            let Arg::Positional(arg) = arg else {
                return Err(bad());
            };
            let ExprKind::Identifier { name } = &arena.expr(*arg).kind else {
                return Err(bad());
            };
            let class = interner.resolve(*name).to_string();
            if varn_core::ErrorClass::of_name(&class).is_none() {
                return Err(LookupFailure::BadDecorator(format!(
                    "`@throws({class})`: `{class}` is not an error class of the canonical tables"
                )));
            }
            if !out.contains(&class) {
                out.push(class);
            }
        }
    }
    Ok(out)
}

fn capability(
    decorators: &[varn_core::ast::Decorator],
    arena: &AstArena,
    interner: &AtomInterner,
) -> Result<Option<String>, LookupFailure> {
    use varn_core::ast::Arg;
    let bad =
        || LookupFailure::BadDecorator("`@permission` takes exactly one string domain".to_string());
    for d in decorators {
        let ExprKind::Call { callee, args, .. } = &arena.expr(d.expression).kind else {
            continue;
        };
        let ExprKind::Identifier { name } = &arena.expr(*callee).kind else {
            continue;
        };
        if ContractDecorator::of(interner.resolve(*name)) != Some(ContractDecorator::Permission) {
            continue;
        }
        let [Arg::Positional(arg)] = args.as_slice() else {
            return Err(bad());
        };
        let ExprKind::StrLiteral { value } = &arena.expr(*arg).kind else {
            return Err(bad());
        };
        return Ok(Some(value.clone()));
    }
    Ok(None)
}

fn class_members(
    decl: &ClassDecl,
    arena: &AstArena,
    interner: &AtomInterner,
) -> Result<Vec<Member>, LookupFailure> {
    let mut out = Vec::new();
    for m in &decl.body {
        match m {
            ClassMember::Method {
                key,
                params: ps,
                return_type,
                modifiers,
                decorators,
                ..
            } => {
                let declared = throws(decorators, arena, interner)?;
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        Kind::StaticMethod
                    } else {
                        Kind::Method
                    },
                    params: params(ps, interner),
                    ret: classify_or(return_type.as_ref(), interner, Mapped::Void),
                    fallible: !declared.is_empty(),
                    throws: declared,
                    capability: capability(decorators, arena, interner)?,
                })
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                decorators,
                ..
            } => {
                let declared = throws(decorators, arena, interner)?;
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        Kind::StaticGetter
                    } else {
                        Kind::Getter
                    },
                    params: Vec::new(),
                    ret: classify_or(return_type.as_ref(), interner, Mapped::Dynamic),
                    fallible: !declared.is_empty(),
                    throws: declared,
                    capability: None,
                })
            }
            ClassMember::Property {
                key,
                type_ann,
                init: None,
                modifiers,
                decorators,
                ..
            } => {
                let declared = throws(decorators, arena, interner)?;
                out.push(Member {
                    symbol: interner.resolve(*key).to_string(),
                    kind: if modifiers.is_static {
                        Kind::StaticGetter
                    } else if modifiers.is_readonly {
                        Kind::Getter
                    } else {
                        Kind::Property
                    },
                    params: Vec::new(),
                    ret: classify_or(type_ann.as_ref(), interner, Mapped::Dynamic),
                    fallible: !declared.is_empty(),
                    throws: declared,
                    capability: None,
                })
            }
            ClassMember::Constructor { params: ps, .. } => out.push(Member {
                symbol: ::varn_core::TokenKind::Constructor.as_str().to_string(),
                kind: Kind::Constructor,
                params: params(ps, interner),
                ret: Mapped::Dynamic,
                fallible: false,
                throws: Vec::new(),
                capability: None,
            }),
            ClassMember::Destructor { .. }
            | ClassMember::Property { .. }
            | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => {}
        }
    }
    Ok(out)
}

fn function_member(
    f: &FunctionDecl,
    arena: &AstArena,
    interner: &AtomInterner,
) -> Result<Member, LookupFailure> {
    Ok(Member {
        symbol: interner.resolve(f.id).to_string(),
        kind: Kind::Function,
        params: params(&f.params, interner),
        ret: classify_or(f.return_type.as_ref(), interner, Mapped::Void),
        fallible: false,
        throws: Vec::new(),
        capability: capability(&f.decorators, arena, interner)?,
    })
}
