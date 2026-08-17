pub(crate) mod advanced;
pub(crate) mod arith;
pub(crate) mod calls;
pub(crate) mod class;
pub(crate) mod collections;
pub(crate) mod compare;
pub(crate) mod ctx;
pub(crate) mod ctx_frames;
pub(crate) mod ctx_json;
pub(crate) mod ctx_modules;
pub(crate) mod ctx_profiling;
pub(crate) mod ctx_tasks;
pub(crate) mod dispatch;
pub mod exceptions;
pub mod frame_ctrl;
pub(crate) mod host;
pub mod host_values;
pub(crate) mod intrinsics;
pub(crate) mod jit_helpers;
pub(crate) mod modules;
pub(crate) mod props;
pub(crate) mod strings;
use crate::value::VmValue;
pub use ctx::ExecCtx;

pub enum VmSuspend {
    Yield {
        value: VmValue,
        dest_reg: u8,
    },
    Await {
        value: varn_types::Value,
        dest_reg: u16,
    },
}

#[derive(Debug, Clone)]
pub enum JitPanic {
    Exception {
        handler: Option<crate::frame::TryHandler>,
        error: VmValue,
        err_obj: Option<crate::error::RuntimeError>,
    },
    Suspend {
        resume_ip: usize,
    },
}
