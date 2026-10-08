use rustc_hash::FxHashSet;
use tower_lsp_f::lsp_types::{
    CodeAction, CodeActionKind, CodeActionResponse, Position, Range, TextEdit, Uri,
};
use varn_core::ast::{AstArena, ClassMember, Decl, Program, StmtId, StmtKind};

use crate::document::DocumentState;

pub fn generate_class_member_actions(
    state: &DocumentState,
    uri: &Uri,
    cursor_line: u32,
) -> Vec<CodeActionResponse> {
    let mut actions = Vec::new();
    let program = match &state.ast {
        Some(p) => p,
        None => return actions,
    };

    let target_class = find_class_at_line(program, &state.ast_arena, cursor_line);
    let class = match target_class {
        Some(c) => c,
        None => return actions,
    };

    let class_name = class.id.map_or("Anonymous", |id| state.name(id));
    let mut fields = Vec::new();
    let mut has_constructor = false;
    let mut methods = FxHashSet::default();

    for member in &class.body {
        match member {
            ClassMember::Property { key, type_ann, .. } => {
                let ty_str = type_ann.as_ref().map_or_else(
                    || varn_core::LangPrimitive::Dynamic.name().to_owned(),
                    |t| state.source_text(t.range).to_owned(),
                );
                fields.push((state.name(*key).to_owned(), ty_str));
            }
            ClassMember::Constructor { .. } => {
                has_constructor = true;
            }
            ClassMember::Method { key, .. } => {
                methods.insert(state.name(*key).to_owned());
            }
            ClassMember::Destructor { .. } | ClassMember::Getter { .. } | ClassMember::Setter { .. }
            | ClassMember::StaticBlock { .. } => {}
        }
    }

    if fields.is_empty() {
        return actions;
    }

    let insert_line = class.range.end.line.saturating_sub(1);
    let insert_pos = Position {
        line: insert_line,
        character: 0,
    };

    if !has_constructor {
        let params = fields
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let assignments = fields
            .iter()
            .map(|(n, _)| format!("        this.{n} = {n};\n"))
            .collect::<String>();

        let ctor_code = format!("    constructor({params}) {{\n{assignments}    }}\n\n");

        let edit = TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: ctor_code,
        };

        let doc_edits = vec![(uri.clone(), vec![edit])];

        actions.push(CodeActionResponse::CodeAction(CodeAction {
            title: format!("Generate constructor for class '{class_name}'"),
            kind: Some(CodeActionKind::RefactorRewrite),
            diagnostics: None,
            edit: Some(crate::features::workspace_edit::doc_edits(doc_edits)),
            command: None,
            is_preferred: Some(false),
            disabled: None,
            tags: None,
            data: None,
        }));
    }

    let ungenerated_fields: Vec<_> = fields
        .iter()
        .filter(|(n, _)| !methods.contains(&format!("get_{n}")) && !methods.contains(n.as_str()))
        .collect();

    if !ungenerated_fields.is_empty() {
        let mut accessors = String::new();
        for (field_name, field_ty) in &ungenerated_fields {
            accessors.push_str(&format!(
                "    get {field_name}(): {field_ty} {{\n        return this.{field_name};\n    }}\n\n"
            ));
            accessors.push_str(&format!(
                "    set {field_name}(value: {field_ty}) {{\n        this.{field_name} = value;\n    }}\n\n"
            ));
        }

        let edit = TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: accessors,
        };

        let doc_edits = vec![(uri.clone(), vec![edit])];

        actions.push(CodeActionResponse::CodeAction(CodeAction {
            title: format!("Generate getters/setters for class '{class_name}'"),
            kind: Some(CodeActionKind::RefactorRewrite),
            diagnostics: None,
            edit: Some(crate::features::workspace_edit::doc_edits(doc_edits)),
            command: None,
            is_preferred: Some(false),
            disabled: None,
            tags: None,
            data: None,
        }));
    }

    actions
}

fn find_class_at_line<'a>(
    program: &Program,
    arena: &'a AstArena,
    line: u32,
) -> Option<&'a varn_core::ast::ClassDecl> {
    for stmt in &program.body {
        if let Some(c) = find_class_in_stmt(arena, *stmt, line) {
            return Some(c);
        }
    }
    None
}

fn find_class_in_stmt(
    arena: &AstArena,
    stmt: StmtId,
    line: u32,
) -> Option<&varn_core::ast::ClassDecl> {
    let stmt = arena.stmt(stmt);
    let s_line = stmt.range.start.line.saturating_sub(1);
    let e_line = stmt.range.end.line;
    if line < s_line || line > e_line {
        return None;
    }

    match &stmt.kind {
        StmtKind::Decl(d) => match d.as_ref() {
            Decl::Class(c) => Some(c),
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
        },
        StmtKind::Block { stmts } => {
            for s in stmts {
                if let Some(c) = find_class_in_stmt(arena, *s, line) {
                    return Some(c);
                }
            }
            None
        }
        StmtKind::Empty
        | StmtKind::Expr { .. }
        | StmtKind::Error
        | StmtKind::If { .. }
        | StmtKind::While { .. }
        | StmtKind::DoWhile { .. }
        | StmtKind::For { .. }
        | StmtKind::ForIn { .. }
        | StmtKind::ForOf { .. }
        | StmtKind::Switch { .. }
        | StmtKind::Return { .. }
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Try { .. }
        | StmtKind::Using { .. }
        | StmtKind::Labeled { .. }
        | StmtKind::Debugger => None,
    }
}
