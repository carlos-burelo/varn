use varn_compiler::FunctionProto;
use varn_types::chunk::Literal;
use varn_types::chunk::PoolEntry;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_utilities::terminal::Section;

pub fn debug_scopes(proto: &FunctionProto, filename: &str) {
    Section::new("static scope tree")
        .subtitle(filename)
        .color(|c| c.magenta())
        .print();

    print_scope_tree(proto, 0);

    Section::new("static scope tree")
        .subtitle("static function scope(s) indexed")
        .close();
}

fn print_scope_tree(proto: &FunctionProto, depth: usize) {
    let indent = "    ".repeat(depth);
    let marker = if depth == 0 {
        "└── "
    } else {
        "    └── "
    };

    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let outer_indent = if depth == 0 {
        ""
    } else {
        &indent[..indent.len() - 4]
    };

    terminal::log(format!(
        "{outer_indent}{marker}{} {} (upvalues: {})",
        chalk("fn").bold(),
        chalk(name).blue(),
        proto.upvalue_count,
    ));

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
        terminal::log(format!(
            "{}    {}",
            indent,
            chalk(format!("const pool strings: {}", ids.join(", "))).dim()
        ));
    }
}
