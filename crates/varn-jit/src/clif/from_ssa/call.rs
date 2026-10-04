//! Calls for the SSA lowering: direct, method, native and canonical invokes.

mod base;
mod direct;
mod invoke;
mod method;
mod native;
mod native_entry;
mod scratch;

pub(super) use base::{emit_call, emit_self_call_framed};
pub(super) use direct::{entry_out_slot, run_entered_or};
pub(super) use invoke::{boxed_window, emit_invoke, emit_new};
pub(super) use method::emit_method_call;
pub(super) use native::emit_call_native_op;
pub(crate) use scratch::ScratchWin;
pub(super) use scratch::{scratch_addr, scratch_max};
