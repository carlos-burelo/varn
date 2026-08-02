pub mod loader;
pub mod provider_impl;
pub mod registry;
mod std_manifest;

pub use provider_impl::{register_embedded_stdlib, register_provider, std_load_error};

pub use loader::CoreSourceLocator;
pub use registry::{is_known, spec_for, MODULE_REGISTRY};
pub use varn_modules::spec::ModuleSpec;

#[cfg(feature = "runtime")]
pub mod dispatch;
#[cfg(feature = "runtime")]
pub mod modules;
#[cfg(feature = "runtime")]
pub mod runtime_ops;

#[cfg(feature = "runtime")]
pub use dispatch::all_native_module_ids;
#[cfg(feature = "runtime")]
pub use dispatch::{describe_op, dispatch_runtime_op, native_op_fn, register_globals_vm, find_native_op_entry};
#[cfg(feature = "runtime")]
pub use modules::build_module;
#[cfg(feature = "runtime")]
pub use modules::globals::set_print_silent;
#[cfg(feature = "runtime")]
pub use modules::has_native_builder;
#[cfg(feature = "runtime")]
pub use modules::testing::{reset_testing_counters, set_testing_silent};
#[cfg(feature = "runtime")]
pub use varn_types::ResourceStore;
