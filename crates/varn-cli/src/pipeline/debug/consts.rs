use varn_compiler::{FunctionProto, Literal, PoolEntry};
use crate::pipeline::colors::{BOLD, DIM, RESET, YELLOW, BLUE, C_CONSTS};

pub fn debug_consts(proto: &FunctionProto, filename: &str) {
    use super::super::colors::{footer, header};
    header(C_CONSTS, "constant pool", filename);
    let mut total = 0usize;
    print_fn_consts(proto, "", &mut total);
    footer(C_CONSTS, &format!("{total} constant(s) total across all functions"));
}

fn print_fn_consts(proto: &FunctionProto, indent: &str, total: &mut usize) {
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    eprintln!("{indent}{BOLD}{BLUE}fn{RESET} {BOLD}{name}{RESET} ({DIM}{} constants{RESET})", proto.chunk.constants.len());
    
    if !proto.chunk.constants.is_empty() {
        eprintln!("{indent}  {DIM}{:<5} │ {:<8} │ Value{RESET}", "Idx", "Kind");
        eprintln!("{indent}  {}", "─".repeat(50));
    }

    for (i, entry) in proto.chunk.constants.iter().enumerate() {
        let (kind, val) = pool_const_parts(entry);
        eprintln!(
            "{indent}  {DIM}[{:03}]{RESET} │ {YELLOW}{:<8}{RESET} │ {BOLD}{}{RESET}",
            i, kind, val
        );
        *total += 1;
    }
    eprintln!();

    let nested_indent = format!("{indent}  ");
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(nested_proto) = entry {
            print_fn_consts(nested_proto, &nested_indent, total);
        }
    }
}

fn pool_const_parts(entry: &PoolEntry) -> (&'static str, String) {
    match entry {
        PoolEntry::Literal(Literal::Null) => ("null", "null".to_owned()),
        PoolEntry::Literal(Literal::Bool(b)) => ("bool", b.to_string()),
        PoolEntry::Literal(Literal::Int(n)) => ("int", n.to_string()),
        PoolEntry::Literal(Literal::Float(f)) => ("float", format!("{f:?}")),
        PoolEntry::Literal(Literal::Str(s)) => ("str", format!("\"{}\"", s)),
        PoolEntry::Literal(Literal::BigInt(n)) => ("bigint", format!("{n}n")),
        PoolEntry::Literal(Literal::Decimal(d)) => ("decimal", format!("{d}d")),
        PoolEntry::Literal(Literal::Symbol(s)) => ("symbol", format!("Symbol({s:?})")),
        PoolEntry::Literal(Literal::Char(c)) => ("char", format!("'{c}'")),
        PoolEntry::Function(p) => {
            let name = p.name.as_deref().unwrap_or("<anon>");
            ("fn", format!("fn {name} (arity={})", p.arity))
        }
    }
}
