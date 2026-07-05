use crate::assembler::Reg;

#[cfg(target_os = "windows")]
pub const ARG_CTX: Reg = Reg::Rcx;
#[cfg(target_os = "windows")]
pub const ARG_CLOSURE: Reg = Reg::Rdx;
#[cfg(target_os = "windows")]
pub const ARG_BASE: Reg = Reg::R8;
#[cfg(target_os = "windows")]
pub const ARG_EXEC_CTX: Reg = Reg::R9;

#[cfg(not(target_os = "windows"))]
pub const ARG_CTX: Reg = Reg::Rdi;
#[cfg(not(target_os = "windows"))]
pub const ARG_CLOSURE: Reg = Reg::Rsi;
#[cfg(not(target_os = "windows"))]
pub const ARG_BASE: Reg = Reg::Rdx;
#[cfg(not(target_os = "windows"))]
pub const ARG_EXEC_CTX: Reg = Reg::Rcx;

pub const REG_RAX: Reg = Reg::Rax;

pub const REG_RCX: Reg = Reg::Rcx;

pub const REG_SCRATCH_1: Reg = Reg::R10;

pub const REG_SCRATCH_2: Reg = Reg::R11;

pub const REG_FRAME_BASE: Reg = Reg::Rbp;

pub const REG_INT_TAG: Reg = Reg::R15;
pub const REG_GLOBALS: Reg = Reg::R14;

pub fn callee_saved_registers() -> &'static [Reg] {
    #[cfg(target_os = "windows")]
    {
        &[
            Reg::Rdi,
            Reg::Rsi,
            Reg::Rbx,
            Reg::Rbp,
            Reg::R12,
            Reg::R13,
            Reg::R14,
            Reg::R15,
        ]
    }
    #[cfg(not(target_os = "windows"))]
    {
        &[Reg::Rbx, Reg::Rbp, Reg::R12, Reg::R13, Reg::R14, Reg::R15]
    }
}
