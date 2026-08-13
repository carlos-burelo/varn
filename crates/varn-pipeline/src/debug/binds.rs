use varn_compiler::{FunctionProto, PoolEntry};
use varn_term::chalk::chalk;
use varn_term::terminal::Section;

pub fn debug_binds(proto: &FunctionProto, filename: &str) {
    Section::new("function hierarchy").subtitle(filename).color(|c| c.blue()).print();
    let mut count = 0usize;
    print_fn_binds(proto, "", true, &mut count);
    varn_term::terminal::log(chalk(format!("── {count} function(s) in hierarchy ──")).dim());
}

fn print_fn_binds(proto: &FunctionProto, indent: &str, is_last: bool, count: &mut usize) {
    let marker = if is_last { "└── " } else { "├── " };
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let mut flags = Vec::new();
    if proto.is_async { flags.push("async"); }
    if proto.is_generator { flags.push("gen"); }
    let flags_str = if flags.is_empty() {
        String::new()
    } else {
        format!(" {}", chalk(format!("({})", flags.join(", "))).dim())
    };

    varn_term::terminal::log(format!(
        "{indent}{marker}{} {} {} {}{}",
        chalk("fn").blue().bold(),
        chalk(name).bold(),
        chalk(format!("arity:{}", proto.arity)).yellow(),
        chalk(format!("upvalues:{}", proto.upvalue_count)).dim(),
        flags_str
    ));
    *count += 1;

    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });
    let nested_fns: Vec<_> = proto.chunk.constants.iter().filter_map(|e| {
        if let PoolEntry::Function(f) = e { Some(f) } else { None }
    }).collect();

    for (i, nested) in nested_fns.iter().enumerate() {
        print_fn_binds(nested, &child_indent, i == nested_fns.len() - 1, count);
    }
}
