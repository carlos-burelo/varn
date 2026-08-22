//! AArch64 / ARM64 (Linux, macOS Apple Silicon, Windows on ARM) setjmp/longjmp.
//!
//! AArch64 AAPCS ABI:
//! - Callee-saved general-purpose registers: X19-X28, X29 (FP), X30 (LR).
//! - Callee-saved floating-point registers: D8-D15.
//! - First argument (_buf) in X0.
//! - Second argument (_val) in W1/X1.
//! - Return value in W0/X0.

#[derive(Default, Debug, Clone, Copy)]
#[repr(C, align(16))]
pub struct JmpBuf {
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub fp: u64, // x29
    pub lr: u64, // x30
    pub sp: u64,
    pub d8: u64,
    pub d9: u64,
    pub d10: u64,
    pub d11: u64,
    pub d12: u64,
    pub d13: u64,
    pub d14: u64,
    pub d15: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn vm_setjmp(_buf: *mut JmpBuf) -> i32 {
    std::arch::naked_asm!(
        "stp x19, x20, [x0, #0]",
        "stp x21, x22, [x0, #16]",
        "stp x23, x24, [x0, #32]",
        "stp x25, x26, [x0, #48]",
        "stp x27, x28, [x0, #64]",
        "stp x29, x30, [x0, #80]",
        "mov x2, sp",
        "str x2, [x0, #96]",
        "stp d8,  d9,  [x0, #104]",
        "stp d10, d11, [x0, #120]",
        "stp d12, d13, [x0, #136]",
        "stp d14, d15, [x0, #152]",
        "mov w0, #0",
        "ret"
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn vm_longjmp(_buf: *const JmpBuf, _val: i32) -> ! {
    std::arch::naked_asm!(
        "ldp x19, x20, [x0, #0]",
        "ldp x21, x22, [x0, #16]",
        "ldp x23, x24, [x0, #32]",
        "ldp x25, x26, [x0, #48]",
        "ldp x27, x28, [x0, #64]",
        "ldp x29, x30, [x0, #80]",
        "ldr x2, [x0, #96]",
        "mov sp, x2",
        "ldp d8,  d9,  [x0, #104]",
        "ldp d10, d11, [x0, #120]",
        "ldp d12, d13, [x0, #136]",
        "ldp d14, d15, [x0, #152]",
        "mov w0, w1",
        "br x30"
    );
}
