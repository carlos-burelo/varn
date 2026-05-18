use varn_compiler::{FunctionProto, PoolEntry};
use crate::pipeline::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_BINDS};

pub fn debug_binds(proto: &FunctionProto, filename: &str) {
    use super::super::colors::{footer, header};
    header(C_BINDS, "function hierarchy", filename);
    let mut count = 0usize;
    print_fn_binds(proto, "", true, &mut count);
    footer(C_BINDS, &format!("{count} function(s) in hierarchy"));
}

fn print_fn_binds(proto: &FunctionProto, indent: &str, is_last: bool, count: &mut usize) {
    let marker = if is_last { "└── " } else { "├── " };
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let mut flags = Vec::new();
    if proto.is_async { flags.push("async"); }
    if proto.is_generator { flags.push("gen"); }
    let flags_str = if flags.is_empty() { String::new() } else { format!(" {DIM}({}){RESET}", flags.join(", ")) };

    eprintln!(
        "{indent}{marker}{BOLD}{BLUE}fn{RESET} {BOLD}{name}{RESET} {YELLOW}arity:{}{RESET} {DIM}upvalues:{}{RESET}{}",
        proto.arity, proto.upvalue_count, flags_str
    );
    *count += 1;

    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });
    let nested_fns: Vec<_> = proto.chunk.constants.iter().filter_map(|e| {
        if let PoolEntry::Function(f) = e { Some(f) } else { None }
    }).collect();

    for (i, nested) in nested_fns.iter().enumerate() {
        print_fn_binds(nested, &child_indent, i == nested_fns.len() - 1, count);
    }
}
