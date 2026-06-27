use varn_opt::{FunctionProto, Literal, PoolEntry};
use varn_core::{IntrinsicType, TypeTag};
use varn_utilities::chalk::chalk;
use varn_utilities::terminal::{self, Section, Table};

pub fn debug_consts(proto: &FunctionProto, filename: &str) {
    Section::new("constant pool").subtitle(filename).color(|c| c.yellow()).print();
    let mut total = 0usize;
    print_fn_consts(proto, "", &mut total);
    varn_utilities::terminal::log(chalk(format!("── {total} constant(s) total across all functions ──")).dim());
}

fn print_fn_consts(proto: &FunctionProto, indent: &str, total: &mut usize) {
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    terminal::log(format!(
        "{}{}{} ({} constants)",
        indent,
        chalk("fn").blue(),
        chalk(name).bold(),
        chalk(proto.chunk.constants.len()).yellow(),
    ));

    if !proto.chunk.constants.is_empty() {
        let mut table = Table::new(["Idx", "Kind", "Value"]);
        for (i, entry) in proto.chunk.constants.iter().enumerate() {
            let (kind, val) = pool_const_parts(entry);
            table.row([
                chalk(format!("[{:03}]", i)).dim().to_string(),
                chalk(kind).yellow().to_string(),
                chalk(val).bold().to_string(),
            ]);
            *total += 1;
        }
        table.print();
    }
    varn_utilities::terminal::blank();

    let nested_indent = format!("{indent}  ");
    for entry in &proto.chunk.constants {
        if let PoolEntry::Function(nested_proto) = entry {
            print_fn_consts(nested_proto, &nested_indent, total);
        }
    }
}

fn pool_const_parts(entry: &PoolEntry) -> (&'static str, String) {
    match entry {
        PoolEntry::Literal(Literal::Null) => (IntrinsicType::Null.as_str(), "null".to_owned()),
        PoolEntry::Literal(Literal::Bool(b)) => (IntrinsicType::Bool.as_str(), b.to_string()),
        PoolEntry::Literal(Literal::Int(n)) => (IntrinsicType::Int.as_str(), n.to_string()),
        PoolEntry::Literal(Literal::Float(f)) => (IntrinsicType::Float.as_str(), format!("{f:?}")),
        PoolEntry::Literal(Literal::Str(s)) => (IntrinsicType::Str.as_str(), format!("\"{}\"", s)),
        PoolEntry::Literal(Literal::BigInt(n)) => (IntrinsicType::BigInt.as_str(), format!("{n}n")),
        PoolEntry::Literal(Literal::Decimal(d)) => {
            (IntrinsicType::Decimal.as_str(), format!("{d}d"))
        }
        PoolEntry::Literal(Literal::Symbol(s)) => {
            (IntrinsicType::Symbol.as_str(), format!("Symbol({s:?})"))
        }
        PoolEntry::Literal(Literal::Char(c)) => (IntrinsicType::Char.as_str(), format!("'{c}'")),
        PoolEntry::Function(p) => {
            let name = p.name.as_deref().unwrap_or("<anon>");
            (TypeTag::Function.name(), format!("fn {name} (arity={})", p.arity))
        }
    }
}
