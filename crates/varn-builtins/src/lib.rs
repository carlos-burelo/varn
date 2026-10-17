#![allow(refining_impl_trait)]
pub mod loader;
pub mod provider_impl;
pub mod registry;
pub mod runtime;

pub use provider_impl::{register_provider, register_std, std_load_error, StdInput};

pub use loader::CoreSourceLocator;
pub use registry::{is_known, spec_for, MODULE_REGISTRY};
pub use varn_modules::spec::ModuleSpec;

#[cfg(feature = "runtime")]
pub mod dispatch;
#[cfg(feature = "runtime")]
pub mod modules;

#[cfg(feature = "runtime")]
pub use dispatch::all_native_module_ids;
#[cfg(feature = "runtime")]
pub use dispatch::{
    check_native_fn_capability, find_native_op_entry, native_op_fn, native_op_id,
    native_op_name_by_fn, register_globals_vm,
};
#[cfg(feature = "runtime")]
pub use modules::build_module;
#[cfg(feature = "runtime")]
pub use modules::has_native_builder;
#[cfg(feature = "runtime")]
pub use modules::testing::reset_testing_counters;
#[cfg(feature = "runtime")]
pub use varn_types::ResourceStore;
