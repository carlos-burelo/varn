//! Prelude-global resolution: rewrite a name-keyed `LoadGlobal` of a
//! native/prelude symbol (`print`, `assert`, …) into `LoadNativeGlobalIdx`.
//!
//! Module globals no longer pass through here: the checker numbers them
//! (`Resolution::GlobalSlot`) and the compiler emits `LoadGlobalIdx` /
//! `StoreGlobalIdx` directly, relative to the module's region base which the
//! running closure carries. What is left is the prelude — a fixed,
//! deterministic layout (`GlobalStore::with_native_layout`) whose indices are
//! the same in every VM, so a proto resolved once needs no per-store rebinding.
//!
//! A name-keyed `LoadGlobal` that does NOT resolve here (a truly dynamic name)
//! stays as it is and the interpreter handles it; `clif` bails on it.

use super::GlobalStore;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::{FunctionProto, PoolEntry};
use varn_types::Literal;

/// Rewrite a name-keyed `LoadGlobal` of a prelude symbol — and, recursively, in
/// every nested proto in its constant pool — to `LoadNativeGlobalIdx`. Module
/// globals are already `LoadGlobalIdx` / `StoreGlobalIdx` from the compiler and
/// pass through untouched.
///
/// Idempotent, and cheap when repeated: a proto already bound to this store
/// returns immediately.
pub fn resolve_in_proto(proto: &mut FunctionProto, globals: &mut GlobalStore) {
    if proto.globals_id.get() == globals.id() {
        return;
    }
    proto.globals_id.set(globals.id());

    // Compiled code bakes the slot indices this pass is about to rewrite, and
    // `Rc::make_mut` hands us a CLONE whose `jit_entry`/`clif_raw` were copied
    // from the original — so without this the clone can run machine code built
    // from the pre-rewrite bytecode, reading another store's slots. Measured at
    // zero on the suite and the benches: it only fires when a rewrite happens.
    proto.jit_entry.set(None);
    proto.clif_raw.set(0);
    *proto.jit_code.borrow_mut() = None;
    proto.jit_failed.set(false);

    let chunk = &mut proto.chunk;
    let mut ip = 0usize;
    while ip < chunk.code.len() {
        // Instruction shapes come from `varn_types::bytecode::decode` and
        // nowhere else; a private length table here would be a second
        // authority that silently drifts.
        let Some(info) = decode(&chunk.code, ip, &chunk.constants) else {
            break;
        };
        // `LoadGlobal dest, name_idx` — dest rides in the opcode word. Only a
        // name that already exists in the store (a prelude symbol) is rewritten;
        // anything else is left for the interpreter's name path.
        if let Some(OpCode::LoadGlobal) = OpCode::from_u8(chunk.code[ip] as u8) {
            if let Some(slot) = slot_for(chunk, ip + 1, globals) {
                let dest = chunk.code[ip] & 0xFF00;
                chunk.code[ip] = dest | (OpCode::LoadNativeGlobalIdx as u8 as u16);
                chunk.code[ip + 1] = slot;
            }
        }
        ip += info.len;
    }

    for entry in &mut chunk.constants {
        if let PoolEntry::Function(nested) = entry {
            resolve_in_proto(std::rc::Rc::make_mut(nested), globals);
        }
    }
}

/// [`resolve_in_proto`] for a shared proto, skipping the `Rc::make_mut` clone
/// when the proto is already bound to `globals`.
///
/// The clone is the expensive half — a `FunctionProto` carries its whole chunk
/// — and it is unavoidable when the work is needed, since a shared proto must
/// never be rewritten in place. Testing the binding FIRST is what keeps a
/// module that is evaluated once per run (the bench harness re-evaluates every
/// local module every iteration) from paying for a rewrite that already
/// happened.
pub fn resolve_shared(proto: &mut std::rc::Rc<FunctionProto>, globals: &mut GlobalStore) {
    if proto.globals_id.get() == globals.id() {
        return;
    }
    resolve_in_proto(std::rc::Rc::make_mut(proto), globals);
}

/// The absolute slot for the name constant at `name_word` when the store
/// already holds it (a prelude symbol). `None` — a name never seen, or an
/// operand that is not a string literal — leaves the instruction name-keyed.
fn slot_for(
    chunk: &varn_types::chunk::Chunk,
    name_word: usize,
    globals: &mut GlobalStore,
) -> Option<u16> {
    let name_idx = *chunk.code.get(name_word)? as usize;
    let PoolEntry::Literal(Literal::Str(name)) = chunk.constants.get(name_idx)? else {
        return None;
    };
    // Only an already-defined name (a prelude/native symbol). A name the store
    // has never seen is genuinely dynamic — leave it name-keyed.
    Some(globals.resolve_index(name)? as u16)
}
