use crate::colors::{footer, header, BLUE, BOLD, C_SCOPE, DIM, RESET};
use varn_compiler::{FunctionProto, PoolEntry};

pub fn debug_scope(proto: &FunctionProto, filename: &str) {
    header(C_SCOPE, "static scope tree", filename);
    let mut count = 0usize;
    print_fn_scope(proto, "", true, &mut count);
    footer(
        C_SCOPE,
        &format!("{count} static function scope(s) indexed"),
    );
}

fn print_fn_scope(proto: &FunctionProto, indent: &str, is_last: bool, count: &mut usize) {
    let marker = if is_last { "└── " } else { "├── " };
    let name = proto.name.as_deref().unwrap_or("<anonymous>");

    eprintln!(
        "{indent}{marker}{BOLD}{BLUE}fn{RESET} {BOLD}{name}{RESET} {DIM}(upvalues: {}){RESET}",
        proto.upvalue_count
    );
    *count += 1;

    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    let locals: Vec<_> = proto
        .chunk
        .constants
        .iter()
        .filter_map(|e| {
            if let varn_compiler::PoolEntry::Literal(varn_compiler::Literal::Str(s)) = e {
                Some(s.as_ref())
            } else {
                None
            }
        })
        .take(8)
        .collect();

    if !locals.is_empty() {
        let suffix = if proto.chunk.constants.len() > 8 {
            " ..."
        } else {
            ""
        };
        eprintln!(
            "{child_indent}{DIM}id: {}{}{RESET}",
            locals.join(", "),
            suffix
        );
    }

    let nested_fns: Vec<_> = proto
        .chunk
        .constants
        .iter()
        .filter_map(|e| {
            if let PoolEntry::Function(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    for (i, nested) in nested_fns.iter().enumerate() {
        print_fn_scope(nested, &child_indent, i == nested_fns.len() - 1, count);
    }
}
