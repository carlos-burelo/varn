pub mod assembler;
pub mod compiler;
pub mod mem;
pub mod registers;
pub mod safepoint;

use std::any::Any;
use std::rc::Rc;
use varn_types::FunctionProto;
use varn_types::VmValue;

/// The platform-neutral JIT function entry point type.
/// Invokes the compiled native code with the ExecCtx, closure, and current stack frame base index.
pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
) -> VmValue;

/// Compiles a bytecode function and returns both the function pointer entry point
/// and the type-erased executable memory buffer (to keep it alive).
pub fn compile(proto: &FunctionProto) -> Result<(JitFn, Rc<dyn Any>), String> {
    let jit_buf = compiler::compile_proto(proto)?;
    let entry_ptr = jit_buf.as_ptr();
    
    // Cast the raw executable pointer to a function pointer
    let jit_fn: JitFn = unsafe { std::mem::transmute(entry_ptr) };
    
    // Wrap the JitBuffer in an Rc<dyn Any> to pass ownership to the VM cleanly
    let jit_code = Rc::new(jit_buf) as Rc<dyn Any>;
    
    Ok((jit_fn, jit_code))
}
