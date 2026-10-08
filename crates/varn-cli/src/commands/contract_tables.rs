use crate::cli::GenContractTablesArgs;
use crate::error::CliError;

pub fn execute(args: GenContractTablesArgs) -> Result<(), CliError> {
    run(&args.crate_dir, &args.out, args.check).map_err(CliError::fatal)
}

use varn_core::ast::{
    AstArena, ClassDecl, ClassMember, Decl, ExportDecl, ExprKind, FunctionDecl, Param, Pattern,
    StmtId, StmtKind, TypeNode,
};
use varn_core::kinds::TypeKind;
use varn_core::{AtomInterner, LangPrimitive};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) struct TableMember {
    pub(crate) symbol: String,
    pub(crate) kind: &'static str,
    pub(crate) params: Vec<(String, bool)>,
    pub(crate) ret: String,
    pub(crate) fallible: bool,
}

fn scalar_code(p: LangPrimitive) -> &'static str {
    match p {
        LangPrimitive::Int => "int",
        LangPrimitive::Float => "float",
        LangPrimitive::Bool => "bool",
        LangPrimitive::Char => "char",
        LangPrimitive::Str => "str",
        LangPrimitive::Void => "void",
        LangPrimitive::Null
        | LangPrimitive::BigInt
        | LangPrimitive::Decimal
        | LangPrimitive::Never
        | LangPrimitive::Dynamic => "dynamic",
    }
}

fn opt_code(inner: String) -> String {
    format!("opt({inner})")
}

fn classify_code(t: &TypeNode, interner: &AtomInterner) -> String {
    match &t.kind {
        TypeKind::Named(n, _) => LangPrimitive::from_str(interner.resolve(*n))
            .map(scalar_code)
            .unwrap_or("dynamic")
            .to_string(),
        TypeKind::Primitive(LangPrimitive::Void) => "void".to_string(),
        TypeKind::TypePredicate { .. } => "bool".to_string(),
        TypeKind::Array(_) => "array".to_string(),
        TypeKind::Union(members) if members.len() == 2 => {
            if matches!(members[1].kind, TypeKind::Primitive(LangPrimitive::Null)) {
                opt_code(classify_code(&members[0], interner))
            } else if matches!(members[0].kind, TypeKind::Primitive(LangPrimitive::Null)) {
                opt_code(classify_code(&members[1], interner))
            } else {
                "dynamic".to_string()
            }
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. } => "dynamic".to_string(),
    }
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

fn unwrap_export<'a>(decl: &'a Decl) -> &'a Decl {
    match decl {
        Decl::Export(ExportDecl::Decl { declaration, .. }) => unwrap_export(declaration),
        other => other,
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
        _ => {}
    });
    Ok(ParsedFile { classes, functions })
}

fn contract_refs(rs_source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = rs_source;
    while let Some(pos) = rest.find("contract:") {
        rest = &rest[pos + "contract:".len()..];
        let trimmed = rest.trim_start();
        if let Some(quoted) = trimmed.strip_prefix('"') {
            if let Some(end) = quoted.find('"') {
                let lit = &quoted[..end];
                if !lit.is_empty() && !lit.contains('\n') && !out.contains(&lit.to_string()) {
                    out.push(lit.to_string());
                }
                rest = &quoted[end.min(quoted.len())..];
            }
        }
    }
    out.sort();
    out
}

fn find_crate_dir_arg(arg: &str) -> Option<PathBuf> {
    if !arg.is_empty() {
        return Some(PathBuf::from(arg));
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("crates/varn-builtins/Cargo.toml").is_file() {
            return Some(dir.join("crates/varn-builtins"));
        }
        if dir.join("Cargo.toml").is_file() && dir.file_name().is_some_and(|n| n == "varn-builtins")
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn file_hash(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    Ok(format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes)))
}

pub(crate) fn generate(crate_dir: &Path) -> Result<String, String> {
    let mut refs = Vec::new();
    let rs_files = rs_file_list(&crate_dir.join("src"));
    for rs in &rs_files {
        let text = std::fs::read_to_string(rs)
            .map_err(|e| format!("cannot read '{}': {e}", rs.display()))?;
        refs.extend(contract_refs(&text));
    }
    refs.sort();
    refs.dedup();
    if refs.is_empty() {
        return Err("no `contract:` references found".to_string());
    }
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut contracts = serde_json::Map::new();
    for r in &refs {
        let path = crate_dir.join(r);
        files.insert(r.clone(), file_hash(&path)?);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        let parsed = parse_contract_file(&source, r)?;
        let mut entry = serde_json::Map::new();
        let mut classes = serde_json::Map::new();
        for (name, members) in &parsed.classes {
            classes.insert(name.clone(), json_members(members));
        }
        entry.insert("classes".to_string(), serde_json::Value::Object(classes));
        entry.insert("functions".to_string(), json_members(&parsed.functions));
        contracts.insert(r.clone(), serde_json::Value::Object(entry));
    }
    let mut root = serde_json::Map::new();
    root.insert(
        "version".to_string(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    );
    root.insert(
        "files".to_string(),
        serde_json::Value::Object(
            files
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect(),
        ),
    );
    root.insert(
        "contracts".to_string(),
        serde_json::Value::Object(contracts),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| format!("cannot serialize contract tables: {e}"))
}

fn json_members(members: &[TableMember]) -> serde_json::Value {
    serde_json::Value::Array(
        members
            .iter()
            .map(|m| {
                serde_json::json!({
                    "symbol": m.symbol,
                    "kind": m.kind,
                    "params": m.params.iter().map(|(ty, rest)| serde_json::json!({"ty": ty, "rest": rest})).collect::<Vec<_>>(),
                    "ret": m.ret,
                    "fallible": m.fallible,
                })
            })
            .collect(),
    )
}

fn rs_file_list(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs_files(dir, &mut out);
    out.sort();
    out
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

pub(crate) fn run(crate_dir_arg: &str, out_arg: &str, check: bool) -> Result<(), String> {
    let crate_dir = find_crate_dir_arg(crate_dir_arg)
        .ok_or_else(|| "cannot locate varn-builtins crate; pass --crate-dir".to_string())?;
    let out = if out_arg.is_empty() {
        crate_dir.join("contracts.json")
    } else {
        PathBuf::from(out_arg)
    };
    let rendered = generate(&crate_dir)?;
    if check {
        let current = std::fs::read_to_string(&out)
            .map_err(|e| format!("cannot read '{}': {e}", out.display()))?;
        if current != rendered {
            return Err(format!(
                "contract tables stale: '{}' differs; run `vn gen-contract-tables`",
                out.display()
            ));
        }
        return Ok(());
    }
    std::fs::write(&out, rendered).map_err(|e| format!("cannot write '{}': {e}", out.display()))?;
    Ok(())
}
