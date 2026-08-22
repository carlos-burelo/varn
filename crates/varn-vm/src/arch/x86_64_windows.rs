//! x86_64 Windows ABI setjmp/longjmp.
//!
//! Windows x86_64 ABI:
//! - Callee-saved general-purpose registers: RDI, RSI, RBX, RBP, R12, R13, R14, R15.
//! - First argument (_buf) in RCX.
//! - Second argument (_val) in EDX/RDX.
//! - Return value in EAX/RAX.

#[derive(Default, Debug, Clone, Copy)]
#[repr(C)]
pub struct JmpBuf {
    pub rdi: u64,
    pub rsi: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn vm_setjmp(_buf: *mut JmpBuf) -> i32 {
    std::arch::naked_asm!(
        "mov [rcx + 0],  rdi",
        "mov [rcx + 8],  rsi",
        "mov [rcx + 16], rbx",
        "mov [rcx + 24], rbp",
        "mov [rcx + 32], r12",
        "mov [rcx + 40], r13",
        "mov [rcx + 48], r14",
        "mov [rcx + 56], r15",
        "lea r10, [rsp + 8]",
        "mov [rcx + 64], r10",
        "mov r10, [rsp]",
        "mov [rcx + 72], r10",
        "xor eax, eax",
        "ret"
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn vm_longjmp(_buf: *const JmpBuf, _val: i32) -> ! {
    std::arch::naked_asm!(
        "mov rdi, [rcx + 0]",
        "mov rsi, [rcx + 8]",
        "mov rbx, [rcx + 16]",
        "mov rbp, [rcx + 24]",
        "mov r12, [rcx + 32]",
        "mov r13, [rcx + 40]",
        "mov r14, [rcx + 48]",
        "mov r15, [rcx + 56]",
        "mov rsp, [rcx + 64]",
        "mov eax, edx",
        "jmp qword ptr [rcx + 72]"
    );
}
