use crate::flags::DebugFlags;
use varn_core::ast::Program;

pub fn debug_types(_program: &Program, _flags: &DebugFlags) {
    eprintln!("debug_types requires varn-lsp which is disabled");
}
