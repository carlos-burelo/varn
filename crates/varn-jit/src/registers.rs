use crate::assembler::Reg;

// --- Platform Calling Convention ABIs ---

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

// --- Scratch and Acumulator Registers ---

/// Primary accumulator register for calculations and return values.
pub const REG_RAX: Reg = Reg::Rax;

/// Secondary accumulator/scratch register.
pub const REG_RCX: Reg = Reg::Rcx;

/// Scratch register 1 (used for temporary addresses, e.g. Stack Data pointer).
pub const REG_SCRATCH_1: Reg = Reg::R10;

/// Scratch register 2 (used for calculated index/offsets).
pub const REG_SCRATCH_2: Reg = Reg::R11;

/// Callee-saved register allocated to cache the absolute frame base address.
pub const REG_FRAME_BASE: Reg = Reg::Rbp;

/// Returns the list of registers that must be preserved (callee-saved)
/// if modified, according to the platform calling convention.
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
        &[
            Reg::Rbx,
            Reg::Rbp,
            Reg::R12,
            Reg::R13,
            Reg::R14,
            Reg::R15,
        ]
    }
}
