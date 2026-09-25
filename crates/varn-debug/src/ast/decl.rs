//! Declarations in the `vn debug ast` outline.

use varn_core::ast::{
    AstArena, ClassMember, Decl, ExportDecl, ExportDefaultDecl, InterfaceMember, VarKind,
};
use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_core::AtomInterner;

use super::expr::{format_pattern, print_expr};
use super::print_stmt;

pub(super) fn print_decl(
    decl: &Decl,
    arena: &AstArena,
    indent: &str,
    is_last: bool,
    interner: &AtomInterner,
) {
    let marker = if is_last { "└── " } else { "├── " };
    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    match decl {
        Decl::Variable(v) => {
            let kind = match v.kind {
                VarKind::Let => "Let",
                VarKind::Const => "Const",
            };
            terminal::log(format!(
                "{indent}{marker}{}",
                chalk(format!("VariableDecl ({kind})")).bold()
            ));
            for (i, d) in v.declarators.iter().enumerate() {
                let d_is_last = i == v.declarators.len() - 1;
                let d_marker = if d_is_last {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{d_marker}{} {}",
                    chalk("Var").bold(),
                    chalk(format_pattern(&d.id, interner)).yellow()
                ));
                if let Some(init) = d.init {
                    let d_ind =
                        format!("{child_indent}{}", if d_is_last { "    " } else { "│   " });
                    print_expr(init, arena, &d_ind, true, interner);
                }
            }
        }
        Decl::Function(f) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("FunctionDecl").bold(),
                chalk(interner.resolve(f.id)).blue()
            ));
            print_stmt(f.body, arena, &child_indent, true, interner);
        }
        Decl::Class(c) => {
            let name = c.id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("ClassDecl").bold(),
                chalk(name).blue()
            ));
            for (i, m) in c.body.iter().enumerate() {
                let is_l = i == c.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    ClassMember::Method { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Method").bold(),
                        chalk(interner.resolve(*key)).blue()
                    )),
                    ClassMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(interner.resolve(*key)).cyan()
                    )),
                    ClassMember::Constructor { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Constructor").bold()))
                    }
                    _ => terminal::log(format!(
                        "{child_indent}{mk}{}",
                        chalk("<other member>").dim()
                    )),
                }
            }
        }
        Decl::Interface(i_node) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("InterfaceDecl").bold(),
                chalk(interner.resolve(i_node.id)).blue()
            ));
            for (idx, m) in i_node.body.iter().enumerate() {
                let is_l = idx == i_node.body.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                match m {
                    InterfaceMember::Property { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Property").bold(),
                        chalk(interner.resolve(*key)).cyan()
                    )),
                    InterfaceMember::Method { key, .. } => terminal::log(format!(
                        "{child_indent}{mk}{} {}",
                        chalk("Method").bold(),
                        chalk(interner.resolve(*key)).blue()
                    )),
                    InterfaceMember::Callable { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Callable").bold()))
                    }
                    InterfaceMember::Index { .. } => {
                        terminal::log(format!("{child_indent}{mk}{}", chalk("Index").bold()))
                    }
                }
            }
        }
        Decl::Enum(e) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("EnumDecl").bold(),
                chalk(interner.resolve(e.id)).blue()
            ));
            for (idx, m) in e.members.iter().enumerate() {
                let is_l = idx == e.members.len() - 1;
                let mk = if is_l { "└── " } else { "├── " };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(m.id)).yellow()
                ));
            }
        }
        Decl::Namespace(n) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("NamespaceDecl").bold(),
                chalk(interner.resolve(n.id)).blue()
            ));
            for (idx, d) in n.body.iter().enumerate() {
                print_decl(d, arena, &child_indent, idx == n.body.len() - 1, interner);
            }
        }
        Decl::TypeAlias(t) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("TypeAlias").bold(),
                chalk(interner.resolve(t.id)).blue()
            ));
        }
        Decl::Import(i) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("Import").bold(),
                chalk(format!("{:?}", interner.resolve(i.source))).yellow()
            ));
        }
        Decl::Export(e) => match e {
            ExportDecl::Decl { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDecl").bold()));
                print_decl(declaration, arena, &child_indent, true, interner);
            }
            ExportDecl::Default { declaration, .. } => {
                terminal::log(format!("{indent}{marker}{}", chalk("ExportDefault").bold()));
                match &**declaration {
                    ExportDefaultDecl::Class(c) => print_decl(
                        &Decl::Class(c.clone()),
                        arena,
                        &child_indent,
                        true,
                        interner,
                    ),
                    ExportDefaultDecl::Function(f) => print_decl(
                        &Decl::Function(f.clone()),
                        arena,
                        &child_indent,
                        true,
                        interner,
                    ),
                    ExportDefaultDecl::Expr(e) => {
                        print_expr(*e, arena, &child_indent, true, interner)
                    }
                }
            }
            ExportDecl::Named { source, .. } => {
                let s = source
                    .map(|s| format!(" from {:?}", interner.resolve(s)))
                    .unwrap_or_default();
                terminal::log(format!(
                    "{indent}{marker}{}{s}",
                    chalk("ExportNamed").bold()
                ));
            }
            ExportDecl::All { source, .. } => {
                terminal::log(format!(
                    "{indent}{marker}{} from {:?}",
                    chalk("ExportAll").bold(),
                    interner.resolve(*source)
                ));
            }
        },
        Decl::Extension(e) => {
            let name = e.id.map(|a| interner.resolve(a)).unwrap_or("<anonymous>");
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("ExtensionDecl").bold(),
                chalk(name).blue()
            ));
        }
        Decl::Struct(s) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("StructDecl").bold(),
                chalk(interner.resolve(s.id)).blue()
            ));
            for (idx, f) in s.fields.iter().enumerate() {
                let mk = if idx == s.fields.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(f.name)).cyan()
                ));
            }
        }
        Decl::SumType(s) => {
            terminal::log(format!(
                "{indent}{marker}{} {}",
                chalk("SumTypeDecl").bold(),
                chalk(interner.resolve(s.id)).blue()
            ));
            for (idx, v) in s.variants.iter().enumerate() {
                let mk = if idx == s.variants.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                terminal::log(format!(
                    "{child_indent}{mk}{}",
                    chalk(interner.resolve(v.name)).yellow()
                ));
            }
        }
    }
}
