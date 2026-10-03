//! Property, index and field access for the SSA lowering.
mod fields;
mod get;
mod index;
mod lengths;
mod set;
mod shared;
pub(super) use fields::{emit_get_fixed_field, emit_set_fixed_field};
pub(super) use get::emit_get_property;
pub(super) use index::{emit_array_push, emit_get_index, emit_set_index};
pub(super) use lengths::{emit_array_length, emit_bytes_length, emit_str_length};
pub(super) use set::emit_set_property;
pub(super) use shared::{emit_this, str_idx};
