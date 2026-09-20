//! Safepoint tracking and root flushes/reloads for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::cell::{Cell, RefCell};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::{RegisterMeta, SlotClass};

use super::super::emit::{
    call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce, unbox_int,
};
use super::super::kinds::K;
use super::super::liveness::Liveness;
use crate::JitHelpers;

/// How precisely an allocation scan reads `OpCode::Intrinsic`.
///
/// The two callers ask different questions of the same opcode list. A whole
/// FUNCTION is scanned to decide whether it needs a frame, safepoints and
/// root flushes, and there the cost of a false `true` is a slower prologue
/// while the cost of a false `false` is a missed root — so it stays
/// conservative. A loop REGION is scanned to decide whether a resolved
/// pointer may be hoisted out of it, and there an `Intrinsic` that provably
/// allocates nothing is the difference between hoisting and not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntrinsicScan {
    /// Every `Intrinsic` counts as allocating.
    Conservative,
    /// An `Intrinsic` counts only when
    /// [`varn_core::intrinsic_ops::intrinsic_allocates`] says its wire byte can
    /// allocate.
    ByWireByte,
}

/// Conservative whole-function scan; see [`IntrinsicScan`].
pub(crate) fn has_alloc(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
) -> Result<bool, String> {
    has_alloc_scan(code, pool, IntrinsicScan::Conservative)
}

pub(crate) fn has_alloc_scan(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    scan: IntrinsicScan,
) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        // The wire byte lives in the high half of the operand word, the same
        // place `strings::emit_str_intrinsic_native` reads it from.
        if scan == IntrinsicScan::ByWireByte
            && OpCode::from_u8(code[ip] as u8) == Some(OpCode::Intrinsic)
            && !varn_core::intrinsic_ops::intrinsic_allocates((code[ip + 1] >> 8) as u8)
        {
            ip += info.len;
            continue;
        }
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(
                OpCode::BuildArray
                    | OpCode::BuildMap
                    | OpCode::BuildTuple
                    | OpCode::BuildObject
                    | OpCode::BuildObjectWithShape
                    | OpCode::BuildRecord
                    | OpCode::ArrayPush
                    | OpCode::ArrayExtend
                    | OpCode::MakeEnumVariant
                    | OpCode::StrConcat
                    | OpCode::BuildStr
                    | OpCode::CallNativeOp
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::Div
                    | OpCode::DivInt
                    | OpCode::Mod
                    | OpCode::Pow
                    | OpCode::BitAnd
                    | OpCode::BitOr
                    | OpCode::BitXor
                    | OpCode::Shl
                    | OpCode::Shr
                    | OpCode::Ushr
                    | OpCode::GetProperty
                    | OpCode::SetProperty
                    | OpCode::Call
                    | OpCode::CallMethod
                    | OpCode::InvokeVirtual
                    | OpCode::ToString
                    | OpCode::Typeof
                    | OpCode::Negate
                    | OpCode::GetSymbol
                    | OpCode::StrSlice
                    | OpCode::Intrinsic
                    | OpCode::MakeClosure
                    | OpCode::MakeClass
                    | OpCode::LoadUpvalue
                    | OpCode::StoreUpvalue
                    | OpCode::CloseUpvalue
                    | OpCode::LoadStaticFn
                    | OpCode::LoadModule
                    | OpCode::LoadModuleSlot
                    | OpCode::StoreModuleSlot
                    | OpCode::GetSuper
                    | OpCode::DeclareField
                    | OpCode::Method
                    | OpCode::DefineStatic
                    | OpCode::DefineGetter
                    | OpCode::DefineSetter
                    | OpCode::DefineStaticGetter
                    | OpCode::DefineStaticSetter
                    | OpCode::Inherit
                    | OpCode::BindMethod
                    | OpCode::Try
                    | OpCode::Throw
                    | OpCode::PopTry
                    | OpCode::Yield
                    | OpCode::Await
                    | OpCode::Spawn
                    | OpCode::ObjectRest
                    | OpCode::ObjectKeys
                    | OpCode::ObjectMerge
                    | OpCode::CallSpread
                    | OpCode::WrapSpread
                    | OpCode::GetIndex
                    | OpCode::SetIndex
                    | OpCode::MapGetIndex
                    | OpCode::MapSetIndex
            )
        ) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

pub(crate) fn has_try(code: &[u16], pool: &[varn_types::chunk::PoolEntry]) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if OpCode::from_u8(code[ip] as u8) == Some(OpCode::Try) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

pub(crate) type SafepointRecord = (usize, Vec<usize>, Vec<usize>);

pub(crate) struct AllocCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub base: cranelift_codegen::ir::Value,
    pub closure: cranelift_codegen::ir::Value,
    pub nregs: usize,
    pub register_meta: &'a [RegisterMeta],
    pub live: &'a Liveness,
    pub narrow_roots: bool,
    pub cur_ip: Cell<usize>,
    pub safepoints: Option<RefCell<Vec<SafepointRecord>>>,
    /// Register → `(class, index)` for this proto, so home slots are addressed
    /// inline (`vec[base[class] + idx]`) without a runtime helper.
    pub layout: varn_types::register_meta::FrameLayout,
}

/// Store a boxed `VmValue` (`I128`; a bare `I64` is treated as an int
/// payload) into register `reg`'s home slot of the current activation,
/// through the `home_store` runtime helper — the class and index come from
/// the VM's `FrameStore`, not from inline address arithmetic.
#[track_caller]
pub(crate) fn store_boxed_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    reg: usize,
    boxed: cranelift_codegen::ir::Value,
) {
    let site = std::panic::Location::caller().line();
    store_boxed_home_at(b, actx, reg, boxed, site);
}

/// Byte offset (from `ExecCtx`) of the `FrameStore` data-pointer word for a
/// class vector.
#[inline]
fn class_ptr_offset(actx: &AllocCtx, class: varn_types::register_meta::SlotClass) -> usize {
    use varn_types::register_meta::SlotClass;
    let fl = &actx.helpers.frame_layout;
    match class {
        SlotClass::Gpr => fl.gpr_ptr_offset,
        SlotClass::Fpr => fl.fpr_ptr_offset,
        SlotClass::Ref => fl.refs_ptr_offset,
        SlotClass::Dyn => fl.dyn_ptr_offset,
    }
}

/// Element size of a class vector.
#[inline]
fn class_elem_size(class: varn_types::register_meta::SlotClass) -> i64 {
    use varn_types::register_meta::SlotClass;
    match class {
        SlotClass::Gpr | SlotClass::Fpr => 8,
        SlotClass::Ref => 4,
        SlotClass::Dyn => 16,
    }
}

/// Inline machine address of register `reg`'s home slot:
/// `class_vec_ptr + (base[class] + idx) * elem_size`. Re-reads the vector data
/// pointer from `ExecCtx` on every access, so a `Vec` reallocation during a
/// call can never leave a stale base.
pub(crate) fn home_addr(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    reg: usize,
) -> cranelift_codegen::ir::Value {
    let class = actx.layout.class_of(reg);
    let idx = actx.layout.idx_of(reg);
    let fl = &actx.helpers.frame_layout;
    let m = MemFlags::trusted();
    let ptr = b
        .ins()
        .load(types::I64, m, actx.exec_ctx, class_ptr_offset(actx, class) as i32);
    let allocs = b
        .ins()
        .load(types::I64, m, actx.exec_ctx, fl.allocs_ptr_offset as i32);
    let byte = b.ins().imul_imm(actx.base, fl.alloc_size as i64);
    let fap = b.ins().iadd(allocs, byte);
    let base_off = (fl.alloc_bases_offset + class.index() * 4) as i32;
    let base = b.ins().uload32(m, fap, base_off);
    let elem = class_elem_size(class);
    let off = b.ins().imul_imm(base, elem);
    let off = b.ins().iadd_imm(off, idx as i64 * elem);
    b.ins().iadd(ptr, off)
}

pub(crate) fn store_boxed_home_at(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    reg: usize,
    boxed: cranelift_codegen::ir::Value,
    _site: u32,
) {
    use varn_types::register_meta::SlotClass;
    let class = actx.layout.class_of(reg);
    let home = home_addr(b, actx, reg);
    let m = MemFlags::trusted();
    match class {
        SlotClass::Gpr => {
            let payload = if b.func.dfg.value_type(boxed) == types::I128 {
                b.ins().isplit(boxed).1
            } else {
                boxed
            };
            b.ins().store(m, payload, home, 0);
        }
        SlotClass::Fpr => {
            let payload = if b.func.dfg.value_type(boxed) == types::I128 {
                b.ins().isplit(boxed).1
            } else {
                boxed
            };
            let f = b.ins().bitcast(types::F64, MemFlags::new(), payload);
            b.ins().store(m, f, home, 0);
        }
        SlotClass::Dyn => {
            let v = if b.func.dfg.value_type(boxed) == types::I128 {
                boxed
            } else {
                let tag = b
                    .ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_INT as i64);
                b.ins().iconcat(tag, boxed)
            };
            b.ins().store(m, v, home, 0);
        }
        SlotClass::Ref => {
            let (tag, payload) = if b.func.dfg.value_type(boxed) == types::I128 {
                b.ins().isplit(boxed)
            } else {
                let tag = b.ins().iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                (tag, boxed)
            };
            let is_null = b.ins().icmp_imm(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                tag,
                varn_types::vm_value::KIND_NULL as i64,
            );
            let low = b.ins().band_imm(payload, 0xFFFF_FFFF);
            let uninit = b
                .ins()
                .iconst(types::I64, varn_types::register_meta::REF_UNINIT as i64);
            let v = b.ins().select(is_null, uninit, low);
            // `Ref` home slots are 4-byte `u32` heap indices (`FrameStore::refs`
            // is `Vec<u32>`); an 8-byte store would overwrite the next slot.
            let v32 = b.ins().ireduce(types::I32, v);
            b.ins().istore32(m, v32, home, 0);
        }
    }
}

pub(crate) fn load_receiver(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
) -> cranelift_codegen::ir::Value {
    load_home(b, actx, 0)
}

/// Read register `reg`'s home slot as a boxed `VmValue` (`I128`), inline.
pub(crate) fn load_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    r: usize,
) -> cranelift_codegen::ir::Value {
    use varn_types::register_meta::SlotClass;
    let class = actx.layout.class_of(r);
    let home = home_addr(b, actx, r);
    let m = MemFlags::trusted();
    match class {
        SlotClass::Gpr => {
            let i = b.ins().load(types::I64, m, home, 0);
            super::super::emit::box_int(b, i)
        }
        SlotClass::Fpr => {
            let f = b.ins().load(types::F64, m, home, 0);
            super::super::emit::box_f64(b, f)
        }
        SlotClass::Dyn => b.ins().load(types::I128, m, home, 0),
        SlotClass::Ref => {
            let idx = b.ins().uload32(m, home, 0);
            let is_uninit = b.ins().icmp_imm(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                idx,
                varn_types::register_meta::REF_UNINIT as i64,
            );
            let null_tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            let heap_tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            let zero = b.ins().iconst(types::I64, 0);
            let tag = b.ins().select(is_uninit, null_tag, heap_tag);
            let payload = b.ins().select(is_uninit, zero, idx);
            b.ins().iconcat(tag, payload)
        }
    }
}

pub(crate) fn box_or_load_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    _state: &[K],
    r: usize,
) -> cranelift_codegen::ir::Value {
    let Some(&var) = actx.vars.get(r) else {
        return super::super::emit::box_null(b);
    };
    let raw = b.use_var(var);
    // CLASS-driven (see `store_home`): a `Ref` register is a heap ref no matter
    // what the `K` lattice thinks locally.
    use varn_types::register_meta::{SlotClass, SlotKind};
    let class = SlotClass::of_kind(
        actx.register_meta
            .get(r)
            .map(|m| m.kind)
            .unwrap_or(SlotKind::Dynamic),
    );
    match class {
        SlotClass::Gpr => super::super::emit::box_int(b, raw),
        SlotClass::Fpr => super::super::emit::box_f64(b, raw),
        // Heap-classed variables already hold the whole VmValue pair.
        SlotClass::Ref | SlotClass::Dyn => b.use_var(var),
    }
}

pub(crate) fn store_home(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    reg: usize,
) {
    let Some(&var) = actx.vars.get(reg) else {
        let null_val = super::super::emit::box_null(b);
        store_boxed_home(b, actx, reg, null_val);
        return;
    };
    let raw = b.use_var(var);
    // CLASS-driven, not `K`-state-driven: the physical class is the checker's
    // proof, while the lowering's `K` lattice can be locally wrong for a
    // register (e.g. a `Ref` receiver confused with a bool). Boxing by class
    // keeps a Ref home a ref. For `Ref`/boxed `Dyn` the home is already current
    // (written on def_result/Move/entry), so the store is skipped.
    let class = varn_types::register_meta::SlotClass::of_kind(
        actx.register_meta
            .get(reg)
            .map(|m| m.kind)
            .unwrap_or(varn_types::register_meta::SlotKind::Dynamic),
    );
    use varn_types::register_meta::SlotClass;
    match class {
        // Heap-classed homes are kept current by def_result/Move/entry.
        SlotClass::Ref | SlotClass::Dyn => {}
        SlotClass::Gpr => {
            let v = super::super::emit::box_int(b, raw);
            store_boxed_home(b, actx, reg, v);
        }
        SlotClass::Fpr => {
            let v = super::super::emit::box_f64(b, raw);
            store_boxed_home(b, actx, reg, v);
        }
    }
    let _ = state;
}

#[track_caller]
pub(crate) fn def_result(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    dest: usize,
    res: cranelift_codegen::ir::Value,
) {
    let site = std::panic::Location::caller().line();
    if std::env::var_os("VARN_HOME_TRACE").is_some() {
        eprintln!(
            "DEFRESULT dest={dest} kind={:?} at={}",
            actx.register_meta.get(dest).map(|m| m.kind),
            std::panic::Location::caller()
        );
    }
    let dest_class = varn_types::register_meta::SlotClass::of_kind(
        actx.register_meta
            .get(dest)
            .map(|m| m.kind)
            .unwrap_or(varn_types::register_meta::SlotKind::Dynamic),
    );
    if matches!(
        dest_class,
        varn_types::register_meta::SlotClass::Ref | varn_types::register_meta::SlotClass::Dyn
    ) {
        // A heap-classed variable carries the whole VmValue.
        let pair = if b.func.dfg.value_type(res) == types::I128 {
            res
        } else {
            let tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            b.ins().iconcat(tag, res)
        };
        b.def_var(actx.vars[dest], pair);
    } else if meta_is_float(actx.register_meta, dest) {
        let f = unbox_f64_coerce(b, res);
        b.def_var(actx.vars[dest], f);
    } else {
        let payload = if b.func.dfg.value_type(res) == types::I128 {
            let (_tag, payload) = b.ins().isplit(res);
            payload
        } else {
            res
        };
        b.def_var(actx.vars[dest], payload);
    }
    store_boxed_home_at(b, actx, dest, res, site);
}

pub(crate) fn emit_backedge_safepoint(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    payload_caches: &[Variable],
) {
    let h = actx.helpers;
    let rcbox = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        h.heap_field_offset as i32,
    );
    let len = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        rcbox,
        h.nursery_len_offset as i32,
    );
    let over = b.ins().icmp_imm(
        IntCC::UnsignedGreaterThanOrEqual,
        len,
        h.nursery_threshold as i64,
    );
    let slow = b.create_block();
    let cont = b.create_block();
    b.ins().brif(over, slow, &[], cont, &[]);

    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    call_helper_void(b, actx.cc, h.gc_safepoint, &[actx.exec_ctx]);
    reload_boxed(b, actx, state, &regs);
    let invalid = b.ins().iconst(types::I64, 0);
    for &cv in payload_caches {
        b.def_var(cv, invalid);
    }
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
}

/// Live registers that can hold a heap reference and therefore must be
/// flushed/reloaded around a collection: physical classes `Ref` and `Dyn`
/// (`Dyn` covers `Bool`/`Str`/`Dynamic`). `Gpr` (raw i64) and `Fpr` (raw f64)
/// never hold a heap index, so a collection cannot change them — this is the
/// "GC roots by construction" half of the class model, replacing the old
/// "everything that is not a float" over-approximation.
pub(crate) fn live_boxed(actx: &AllocCtx, state: &[K]) -> Vec<usize> {
    let ip = actx.cur_ip.get();
    let is_root_class = |r: usize| {
        let kind = actx
            .register_meta
            .get(r)
            .map(|m| m.kind)
            .unwrap_or(varn_types::register_meta::SlotKind::Dynamic);
        matches!(
            SlotClass::of_kind(kind),
            SlotClass::Ref | SlotClass::Dyn
        )
    };
    let live_root = (0..actx.nregs)
        .filter(|&r| is_root_class(r))
        .filter(|&r| actx.live.is_live_after(ip, r));
    let rooted = |r: usize| !actx.narrow_roots || state.get(r).copied().is_none_or(is_root_kind);
    let regs: Vec<usize> = live_root.clone().filter(|&r| rooted(r)).collect();
    if let Some(rec) = &actx.safepoints {
        let unboxed: Vec<usize> = live_root.filter(|&r| !rooted(r)).collect();
        rec.borrow_mut().push((ip, regs.clone(), unboxed));
    }
    regs
}

fn is_root_kind(k: K) -> bool {
    !matches!(k, K::Int | K::Bool)
}

pub(crate) fn flush_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    for &r in regs {
        store_home(b, actx, state, r);
    }
}

pub(crate) fn reload_boxed(b: &mut FunctionBuilder, actx: &AllocCtx, state: &[K], regs: &[usize]) {
    for &r in regs {
        let v = load_home(b, actx, r);
        let class = varn_types::register_meta::SlotClass::of_kind(
            actx.register_meta
                .get(r)
                .map(|m| m.kind)
                .unwrap_or(varn_types::register_meta::SlotKind::Dynamic),
        );
        if matches!(
            class,
            varn_types::register_meta::SlotClass::Ref | varn_types::register_meta::SlotClass::Dyn
        ) {
            // Heap-classed variables hold the whole VmValue.
            b.def_var(actx.vars[r], v);
        } else if meta_is_float(actx.register_meta, r) {
            let f = unbox_f64_coerce(b, v);
            b.def_var(actx.vars[r], f);
        } else {
            let restored = match state[r] {
                K::Int => unbox_int(b, v),
                K::Bool => unbox_bool(b, v),
                _ => {
                    let (_tag, payload) = b.ins().isplit(v);
                    payload
                }
            };
            b.def_var(actx.vars[r], restored);
        }
    }
}
