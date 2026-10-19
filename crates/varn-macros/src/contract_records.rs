use varn_core::ast::{ClassDecl, ClassMember, Decl, InterfaceDecl, InterfaceMember, StmtKind};
use varn_core::AtomInterner;

use crate::contract_parse::{classify, unwrap_export};
use crate::varn_contract::Mapped;

pub(crate) enum RecordSource {
    Interface,
    Class,
}

type Fields = Vec<(String, Mapped)>;

pub(crate) fn record_fields(
    source: &str,
    path: &str,
    kind: &RecordSource,
    name: &str,
) -> Result<Fields, String> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, path);
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, path, AtomInterner::new())
            .map_err(|_| format!("cannot parse contract `{path}`"))?;
    for &stmt in &program.body {
        let StmtKind::Decl(decl) = &arena.stmt(stmt).kind else {
            continue;
        };
        let decl = unwrap_export(decl);
        let found = match kind {
            RecordSource::Interface => {
                let Decl::Interface(i) = decl else {
                    continue;
                };
                if interner.resolve(i.id) != name {
                    continue;
                }
                interface_fields(i, name, &interner)
            }
            RecordSource::Class => {
                let Decl::Class(c) = decl else {
                    continue;
                };
                if !c.id.is_some_and(|id| interner.resolve(id) == name) {
                    continue;
                }
                Ok(class_fields(c, &interner))
            }
        };
        return found;
    }
    Err(format!("`{name}` not found in contract `{path}`"))
}

fn interface_fields(
    i: &InterfaceDecl,
    name: &str,
    interner: &AtomInterner,
) -> Result<Fields, String> {
    let mut fields = Vec::new();
    for member in &i.body {
        match member {
            InterfaceMember::Property { key, type_ann, .. } => fields.push((
                interner.resolve(*key).to_string(),
                classify(type_ann, interner),
            )),
            InterfaceMember::Method { .. }
            | InterfaceMember::Index { .. }
            | InterfaceMember::Callable { .. } => return Err(format!(
                "interface `{name}` declares a non-field member; a native record holds fields only"
            )),
        }
    }
    Ok(fields)
}

fn class_fields(c: &ClassDecl, interner: &AtomInterner) -> Fields {
    c.body
        .iter()
        .filter_map(|member| match member {
            ClassMember::Property { key, type_ann, .. } => Some((
                interner.resolve(*key).to_string(),
                type_ann
                    .as_ref()
                    .map(|t| classify(t, interner))
                    .unwrap_or(Mapped::Dynamic),
            )),
            ClassMember::Constructor { .. }
            | ClassMember::Destructor { .. }
            | ClassMember::Method { .. }
            | ClassMember::Getter { .. }
            | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => None,
        })
        .collect()
}
