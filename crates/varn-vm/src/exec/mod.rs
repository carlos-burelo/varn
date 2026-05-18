pub mod advanced;
pub mod arith;
pub mod calls;
pub mod class;
pub mod collections;
pub mod compare;
pub mod control;
pub mod ctx;
pub mod dispatch;
pub mod exceptions;
pub mod frame_ctrl;
pub mod modules;
pub mod props;
pub mod strings;
pub mod vars;

pub use ctx::ExecCtx;

use crate::value::VmValue;
use varn_types::generator::GenChannel;

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
