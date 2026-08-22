//! x86_64 System V ABI (Linux, macOS, BSD) setjmp/longjmp.
//!
//! System V AMD64 ABI:
//! - Callee-saved general-purpose registers: RBX, RBP, R12, R13, R14, R15.
//! - First argument (_buf) in RDI.
//! - Second argument (_val) in ESI/RSI.
//! - Return value in EAX/RAX.

#[derive(Default, Debug, Clone, Copy)]
#[repr(C)]
pub struct JmpBuf {
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
        "mov [rdi + 0],  rbx",
        "mov [rdi + 8],  rbp",
        "mov [rdi + 16], r12",
        "mov [rdi + 24], r13",
        "mov [rdi + 32], r14",
        "mov [rdi + 40], r15",
        "lea r10, [rsp + 8]",
        "mov [rdi + 48], r10",
        "mov r10, [rsp]",
        "mov [rdi + 56], r10",
        "xor eax, eax",
        "ret"
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn vm_longjmp(_buf: *const JmpBuf, _val: i32) -> ! {
    std::arch::naked_asm!(
        "mov rbx, [rdi + 0]",
        "mov rbp, [rdi + 8]",
        "mov r12, [rdi + 16]",
        "mov r13, [rdi + 24]",
        "mov r14, [rdi + 32]",
        "mov r15, [rdi + 40]",
        "mov rsp, [rdi + 48]",
        "mov eax, esi",
        "jmp qword ptr [rdi + 56]"
    );
}
