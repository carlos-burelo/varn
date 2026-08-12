//! Global-slot resolution: rewrite name-keyed global access into index-keyed
//! access, once, before a proto ever runs.
//!
//! `LoadGlobal`/`StoreGlobal`/`DefineGlobal` carry a constant-pool index for
//! the global's NAME — reading one costs a hash lookup in the interpreter and
//! a full FFI helper call (with a register flush/reload around it) in the JIT.
//! `LoadGlobalIdx`/`StoreGlobalIdx`/`DefineGlobalIdx` carry the slot index
//! instead: one indexed load or store, inline, in both tiers.
//!
//! Slot indices belong to ONE [`GlobalStore`], and every `Vm` has its own —
//! isolates define `isIsolate` before loading anything, so the same name lands
//! on different indices in different VMs. A proto must therefore be resolved
//! against the store it will actually run against, and a proto shared between
//! VMs (the thread-local stdlib cache, the `precompiled` map) must never be
//! resolved in place. Both call sites go through `Rc::make_mut`, which clones
//! precisely when the proto is shared — that is the whole safety argument.
//!
//! Two call sites cover every proto that runs. `ExecCtx::eval_module_proto`
//! takes every module — in every VM, from `precompiled` or from a
//! `ModuleLoader`, main thread or isolate worker. [`crate::Vm::resolve_globals`]
//! takes the entry proto, the one that never passes through there; the pipeline
//! calls it during setup rather than from `Vm::run`, which the bench harness
//! times. Nested protos come along through the constant pool.
//!
//! Because that is airtight, `clif` lowers ONLY the `*Idx` forms — see
//! `varn_jit::clif::globals`. Break it and those functions silently drop to the
//! interpreter.

use super::GlobalStore;
use crate::value::VmValue;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::{FunctionProto, PoolEntry};
use varn_types::Literal;

/// Rewrite every name-keyed global access in `proto` — and, recursively, in
/// every nested proto in its constant pool — to the `*Idx` form, defining
/// slots in `globals` as they are first seen.
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
        match OpCode::from_u8(chunk.code[ip] as u8) {
            // `LoadGlobal dest, name_idx` — dest rides in the opcode word.
            Some(OpCode::LoadGlobal) => {
                if let Some(slot) = slot_for(chunk, ip + 1, globals) {
                    let dest = chunk.code[ip] & 0xFF00;
                    chunk.code[ip] = dest | (OpCode::LoadGlobalIdx as u8 as u16);
                    chunk.code[ip + 1] = slot;
                }
            }
            // `StoreGlobal|DefineGlobal src, name_idx` — src is in the first
            // operand word, the name constant in the second.
            Some(op @ (OpCode::StoreGlobal | OpCode::DefineGlobal)) => {
                if let Some(slot) = slot_for(chunk, ip + 2, globals) {
                    let new_op = if op == OpCode::StoreGlobal {
                        OpCode::StoreGlobalIdx
                    } else {
                        OpCode::DefineGlobalIdx
                    };
                    chunk.code[ip] = new_op as u8 as u16;
                    chunk.code[ip + 2] = slot;
                }
            }
            _ => {}
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

/// The slot index for the name constant at `name_word`, defining it if this is
/// its first sighting. `None` leaves the instruction in its name-keyed form —
/// only reachable if the operand does not point at a string literal, which the
/// emitter never produces.
fn slot_for(
    chunk: &varn_types::chunk::Chunk,
    name_word: usize,
    globals: &mut GlobalStore,
) -> Option<u16> {
    let name_idx = *chunk.code.get(name_word)? as usize;
    let PoolEntry::Literal(Literal::Str(name)) = chunk.constants.get(name_idx)? else {
        return None;
    };
    let slot = globals
        .resolve_index(name)
        .unwrap_or_else(|| globals.define(name, VmValue::null()));
    Some(slot as u16)
}
