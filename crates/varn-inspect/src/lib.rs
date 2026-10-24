mod dashboard;
mod expect;
mod interact;
mod queries;
mod types;

use varn_debug::flags::DebugFlags;

pub fn run_for(path: &str, eval: Option<&str>, flags: &DebugFlags) {
    if !(flags.types || flags.lsp) {
        return;
    }
    let source = match eval {
        Some(code) => code.to_owned(),
        None => std::fs::read_to_string(path).unwrap_or_default(),
    };
    if flags.types {
        types::debug_types(path, &source, flags);
    }
    if flags.lsp {
        dashboard::debug_lsp(path, &source, flags);
    }
    if flags.lsp_interact {
        interact::debug_interact(path, &source, flags);
    }
}
