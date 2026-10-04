//! Inline access to a register's home slot in the partitioned `FrameStore`.
//!
//! A home is the register's slot in its class vector (`gpr`, `fpr`, `refs`,
//! `dyn_`): `vec_ptr + (base[class] + idx) * elem_size`, read straight out of
//! the live `ExecCtx`. Both lowerings — from bytecode and from typed SSA —
//! reach homes through here, so the address arithmetic and the per-class
//! conversions exist once. Every access re-reads the vector pointer from
//! `ExecCtx`, so a reallocation during a call never leaves a stale base.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::{FrameLayout, SlotClass};
use varn_types::vm_value::{KIND_HEAP, KIND_INT, KIND_NULL};

use crate::JitFrameLayout;

/// What an inline home access needs: the live `ExecCtx`, this activation's
/// id, the register-to-class map, and the `FrameStore` field offsets.
pub(crate) struct Homes<'a> {
    pub exec_ctx: Value,
    pub base: Value,
    pub layout: &'a FrameLayout,
    pub offsets: &'a JitFrameLayout,
}

impl Homes<'_> {
    /// Byte offset (from `ExecCtx`) of `class`'s vector data pointer.
    fn class_ptr_offset(&self, class: SlotClass) -> i32 {
        let fl = self.offsets;
        (match class {
            SlotClass::Gpr => fl.gpr_ptr_offset,
            SlotClass::Fpr => fl.fpr_ptr_offset,
            SlotClass::Ref => fl.refs_ptr_offset,
            SlotClass::Dyn => fl.dyn_ptr_offset,
        }) as i32
    }

    /// Machine address of register `reg`'s home slot.
    pub(crate) fn addr(&self, b: &mut FunctionBuilder, reg: usize) -> Value {
        let class = self.layout.class_of(reg);
        let idx = self.layout.idx_of(reg);
        let fl = self.offsets;
        let m = cranelift_codegen::ir::MemFlagsData::trusted();
        let ptr = b
            .ins()
            .load(types::I64, m, self.exec_ctx, self.class_ptr_offset(class));
        let allocs = b
            .ins()
            .load(types::I64, m, self.exec_ctx, fl.allocs_ptr_offset as i32);
        let byte = b.ins().imul_imm_u(self.base, fl.alloc_size as i64);
        let fap = b.ins().iadd(allocs, byte);
        let base_off = (fl.alloc_bases_offset + class.index() * 4) as i32;
        let base = b.ins().uload32(m, fap, base_off);
        let elem = elem_size(class);
        let off = b.ins().imul_imm_u(base, elem);
        let off = b.ins().iadd_imm_u(off, idx as i64 * elem);
        b.ins().iadd(ptr, off)
    }

    /// Write `value` to the precomputed home address `home` (see
    /// [`addr`][Self::addr]), converted to the home's class. `value`
    /// is a boxed `VmValue` (`I128`), or a bare `I64` payload: an `int` for a
    /// `Gpr`/`Dyn` home, an object address (0 = null) for a `Ref` one, raw `f64` bits for an
    /// `Fpr` one.
    pub(crate) fn store_at(&self, b: &mut FunctionBuilder, home: Value, reg: usize, value: Value) {
        let class = self.layout.class_of(reg);
        let m = cranelift_codegen::ir::MemFlagsData::trusted();
        let boxed = b.func.dfg.value_type(value) == types::I128;
        match class {
            SlotClass::Gpr => {
                let payload = if boxed {
                    b.ins().isplit(value).1
                } else {
                    value
                };
                b.ins().store(m, payload, home, 0);
            }
            SlotClass::Fpr => {
                let payload = if boxed {
                    b.ins().isplit(value).1
                } else {
                    value
                };
                let f = b.ins().bitcast(
                    types::F64,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    payload,
                );
                b.ins().store(m, f, home, 0);
            }
            SlotClass::Dyn => {
                let v = if boxed {
                    value
                } else {
                    let tag = b.ins().iconst(types::I64, KIND_INT as i64);
                    b.ins().iconcat(tag, value)
                };
                b.ins().store(m, v, home, 0);
            }
            SlotClass::Ref => {
                let (tag, payload) = if boxed {
                    b.ins().isplit(value)
                } else {
                    (b.ins().iconst(types::I64, KIND_HEAP as i64), value)
                };
                let is_null = b.ins().icmp_imm_u(IntCC::Equal, tag, KIND_NULL as i64);
                let zero = b.ins().iconst(types::I64, 0);
                let v = b.ins().select(is_null, zero, payload);
                b.ins().store(m, v, home, 0);
            }
        }
    }

    /// Read `reg`'s home as a boxed `VmValue` (`I128`).
    pub(crate) fn load(&self, b: &mut FunctionBuilder, reg: usize) -> Value {
        let home = self.addr(b, reg);
        self.load_at(b, home, reg)
    }

    /// [`load`] from a precomputed [`addr`][Self::addr].
    pub(crate) fn load_at(&self, b: &mut FunctionBuilder, home: Value, reg: usize) -> Value {
        let class = self.layout.class_of(reg);
        let m = cranelift_codegen::ir::MemFlagsData::trusted();
        match class {
            SlotClass::Gpr => {
                let i = b.ins().load(types::I64, m, home, 0);
                super::emit::box_int(b, i)
            }
            SlotClass::Fpr => {
                let f = b.ins().load(types::F64, m, home, 0);
                super::emit::box_f64(b, f)
            }
            SlotClass::Dyn => b.ins().load(types::I128, m, home, 0),
            SlotClass::Ref => {
                let addr = b.ins().load(types::I64, m, home, 0);
                let is_null = b.ins().icmp_imm_u(IntCC::Equal, addr, 0);
                let null_tag = b.ins().iconst(types::I64, KIND_NULL as i64);
                let heap_tag = b.ins().iconst(types::I64, KIND_HEAP as i64);
                let tag = b.ins().select(is_null, null_tag, heap_tag);
                b.ins().iconcat(tag, addr)
            }
        }
    }
}

/// Element size of a class vector.
fn elem_size(class: SlotClass) -> i64 {
    match class {
        SlotClass::Gpr | SlotClass::Fpr => 8,
        SlotClass::Ref => 8,
        SlotClass::Dyn => 16,
    }
}
