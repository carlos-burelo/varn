pub mod cache;
mod check;
mod class_link;
mod compilation;
mod compile;
mod core;
mod error;
mod execute;
mod executable;
pub mod fmt;
pub mod import_collector;
mod layer_link;
mod lex;
mod lower;
pub mod module_precompile;
mod parse;
mod policy;
pub mod portable;
mod quiet_parse;
mod request;
mod session;
mod source;
pub mod stdlib_loader;

pub use varn_resolver::ModuleResolver;

pub use check::check_program as phase_check;
pub use class_link::verify_foreign_layouts;
pub use compilation::{CacheStatus, Compilation, Stage};
pub use core::core_protos_owned;
pub use error::{DiagnosticReport, Failure, RuntimeFailure};
pub use execute::{boot_machine, enter_main, execute};
pub use executable::Executable;
pub use layer_link::verify_synthetic_imports;
pub use lex::{scan as phase_lex, Tokens};
pub use lower::{
    compile_tir, emit_tir, inject_star_reexports, lower_module, report_export_collisions,
    resolve_import_slots, sorted_export_names, tir_import_sources, LowerInput, Lowered,
};
pub use parse::{parse as phase_parse, Parsed};
pub use policy::Policy;
pub use request::{CachePolicy, Goal, Request};
pub use session::Session;
pub use source::Source;
pub use varn_core::debug_flags::DebugFlags;
pub use varn_project::Anchor;
pub use varn_types::capabilities::CapabilitySet;
