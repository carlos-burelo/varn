use varn_opt::{FunctionProto, PoolEntry};
use varn_utilities::chalk::chalk;
use varn_utilities::terminal::Section;

pub fn debug_scope(proto: &FunctionProto, filename: &str) {
    Section::new("static scope tree").subtitle(filename).color(|c| c.magenta()).print();
    let mut count = 0usize;
    print_fn_scope(proto, "", true, &mut count);
    varn_utilities::terminal::log(
        chalk(format!("── {count} static function scope(s) indexed ──")).dim(),
    );
}

fn print_fn_scope(proto: &FunctionProto, indent: &str, is_last: bool, count: &mut usize) {
    let marker = if is_last { "└── " } else { "├── " };
    let name = proto.name.as_deref().unwrap_or("<anonymous>");

    varn_utilities::terminal::log(format!(
        "{indent}{marker}{} {} {}",
        chalk("fn").blue().bold(),
        chalk(name).bold(),
        chalk(format!("(upvalues: {})", proto.upvalue_count)).dim(),
    ));
    *count += 1;

    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });

    let locals: Vec<_> = proto
        .chunk
        .constants
        .iter()
        .filter_map(|e| {
            if let varn_opt::PoolEntry::Literal(varn_opt::Literal::Str(s)) = e {
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
        varn_utilities::terminal::log(format!(
            "{}{}",
            child_indent,
            chalk(format!("id: {}{}", locals.join(", "), suffix)).dim(),
        ));
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
