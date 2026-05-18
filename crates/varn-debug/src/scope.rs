use crate::colors::{BLUE, BOLD, C_SCOPE, DIM, RESET};
use varn_compiler::FunctionProto;
use varn_types::chunk::Literal;
use varn_types::chunk::PoolEntry;

pub fn debug_scopes(proto: &FunctionProto, filename: &str) {
    use crate::colors::{footer, header};
    header(C_SCOPE, "static scope tree", filename);

    print_scope_tree(proto, 0);

    footer(C_SCOPE, "static function scope(s) indexed");
}

fn print_scope_tree(proto: &FunctionProto, depth: usize) {
    let indent = "    ".repeat(depth);
    let marker = if depth == 0 {
        "└── "
    } else {
        "    └── "
    };

    let name = proto.name.as_deref().unwrap_or("<anonymous>");

    eprintln!(
        "{indent}{marker}{BOLD}fn{RESET} {BLUE}{}{RESET} (upvalues: {})",
        name,
        proto.upvalue_count,
        indent = if depth == 0 {
            ""
        } else {
            &indent[..indent.len() - 4]
        },
        marker = marker,
        BOLD = BOLD,
        RESET = RESET,
        BLUE = BLUE
    );

    let ids: Vec<_> = proto
        .chunk
        .constants
        .iter()
        .filter_map(|c| {
            if let PoolEntry::Literal(Literal::Str(s)) = c {
                Some(s.as_ref())
            } else {
                None
            }
        })
        .collect();

    if !ids.is_empty() {
        eprintln!(
            "{indent}    {DIM}const pool strings: {}{RESET}",
            ids.join(", "),
            indent = indent,
            DIM = DIM,
            RESET = RESET
        );
    }
}
