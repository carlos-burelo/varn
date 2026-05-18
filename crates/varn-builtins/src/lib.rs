pub mod loader;
pub mod registry;

pub use loader::ModuleLoader;
pub use registry::{is_known, spec_for, MODULE_REGISTRY};

#[cfg(feature = "runtime")]
pub mod dispatch;
#[cfg(feature = "runtime")]
pub mod host_ops;
#[cfg(feature = "runtime")]
pub mod modules;
#[cfg(feature = "runtime")]
pub mod resource;

#[cfg(feature = "runtime")]
pub use dispatch::{describe_op, dispatch_host_op, register_globals_vm};
#[cfg(feature = "runtime")]
pub use modules::build_module;
#[cfg(feature = "runtime")]
pub use modules::has_native_builder;
#[cfg(feature = "runtime")]
pub use modules::print::set_print_silent;
#[cfg(feature = "runtime")]
pub use modules::testing::{reset_testing_counters, set_testing_silent};
#[cfg(feature = "runtime")]
pub use resource::ResourceStore;
