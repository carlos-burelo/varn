pub mod advanced;
pub mod arith;
pub mod calls;
pub mod class;
pub mod collections;
pub mod compare;
pub mod control;
pub mod ctx;
pub mod ctx_frames;
pub mod ctx_jit_runtime;
pub mod ctx_jit_values;
pub mod ctx_modules;
pub mod ctx_profiling;
pub mod ctx_tasks;
pub mod dispatch;
pub mod exceptions;
pub mod frame_ctrl;
pub mod host_values;
pub mod intrinsics;
pub mod modules;
pub mod props;
pub mod strings;
pub mod vars;
use crate::value::VmValue;
pub use ctx::ExecCtx;

pub enum VmSuspend {
    Yield {
        value: VmValue,
        dest_reg: u8,
    },
    Task(varn_types::AsyncTask),

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
