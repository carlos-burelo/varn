use crate::flags::DebugFlags;
use varn_core::ast::Program;
use varn_term::terminal;

pub fn debug_types(_program: &Program, _flags: &DebugFlags) {
    terminal::log("debug_types requires varn-lsp which is disabled");
}
