//! Single emitter for the JIT → runtime-helper calling ceremony.
//!
//! Every FFI call site shares the same shape: spill allocated registers,
//! save the live ABI registers, align the stack, marshal `(ExecCtx, args…)`
//! into the platform argument registers, call, restore, store the result
//! and reload. This used to be copy-pasted ~100 times across codegen; any
//! ABI change meant a 100-site edit. Call sites now declare intent through
//! [`FfiCallSpec`] and this module owns the instruction sequence.
//!
//! Argument slots 1–4 are `ARG_CTX`, `ARG_CLOSURE`, `ARG_BASE`,
//! `ARG_EXEC_CTX` on both Windows and SysV (the aliases resolve to the
//! right physical registers per platform). Slot 1 always carries the
//! ExecCtx pointer, taken from `ARG_EXEC_CTX` before it is overwritten.

use crate::assembler::{Assembler, Reg};
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all_except, emit_store, RegMap,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX, REG_FRAME_BASE};

/// One helper argument beyond the implicit leading ExecCtx pointer.
pub(crate) enum FfiArg {
    /// Virtual register, loaded through the regmap (physical copy or
    /// frame slot).
    Vreg(usize),
    /// Immediate 64-bit constant.
    Imm(u64),
    /// The current closure pointer, spilled by the prologue at `[Rsp+8]`.
    SavedClosure,
}

pub(crate) struct FfiCallSpec<'a> {
    /// Address of the `extern "C"` helper.
    pub helper: usize,
    /// Up to three value arguments, passed after the ExecCtx pointer.
    pub args: &'a [FfiArg],
    /// Spill allocated registers to their frame slots before the call.
    /// Required whenever the helper may read or write frame registers or
    /// trigger GC; skip only for helpers that touch neither.
    pub flush: bool,
    /// Store the helper's return value (Rax) into this virtual register.
    pub dest: Option<usize>,
    /// Reload allocated registers after the call (dest excluded).
    pub reload: bool,
    /// The helper may grow the VM stack and move the frame: recompute
    /// `REG_FRAME_BASE` from the ExecCtx after the call.
    pub recompute_frame: bool,
}

pub(crate) fn emit_ffi_call(asm: &mut Assembler, regmap: &RegMap, spec: &FfiCallSpec) {
    debug_assert!(spec.args.len() <= 3);

    if spec.flush {
        emit_flush_all(asm, regmap);
    }

    // Value args go through scratch registers first: vreg loads only read
    // callee-saved state (alloc regs / frame memory), never the ABI
    // registers we are about to save and overwrite. Loading happens BEFORE
    // the pushes so `SavedClosure` sees the prologue's `[Rsp+8]` slot.
    const SCRATCH: [Reg; 3] = [Reg::Rax, Reg::R10, Reg::R11];
    for (i, arg) in spec.args.iter().enumerate() {
        match arg {
            FfiArg::Vreg(v) => emit_load(asm, SCRATCH[i], *v, regmap),
            FfiArg::Imm(x) => asm.mov_reg_imm64(SCRATCH[i], *x),
            FfiArg::SavedClosure => asm.mov_reg_mem(SCRATCH[i], Reg::Rsp, 8),
        }
    }

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    // ExecCtx into slot 1 first — slot 4 IS `ARG_EXEC_CTX`, so it must be
    // read before a third value argument overwrites it.
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    let slots = [ARG_CLOSURE, ARG_BASE, ARG_EXEC_CTX];
    for i in 0..spec.args.len() {
        asm.mov_reg_reg(slots[i], SCRATCH[i]);
    }

    asm.mov_reg_imm64(Reg::R10, spec.helper as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    // The return value must survive the pops below; R11 is volatile and
    // not part of the restore sequence.
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    if spec.recompute_frame {
        asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
        asm.mov_reg_reg(REG_FRAME_BASE, ARG_BASE);
        asm.shl_reg_imm8(REG_FRAME_BASE, 3);
        asm.add_reg_reg(REG_FRAME_BASE, ARG_CTX);
    }

    if let Some(d) = spec.dest {
        emit_store(asm, Reg::R11, d, regmap);
    }
    if spec.reload {
        emit_reload_all_except(asm, regmap, spec.dest);
    }
}
