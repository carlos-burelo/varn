//! The allocation scan and the back-edge poll shared by the lowering.
//!
//! The whole-function scan (`has_alloc`) feeds the size gate's leaf-safe
//! rule; the poll keeps an allocating loop collecting. Everything else that
//! lived here (activation contexts, home flushes, live sets, stack-map
//! records) served the bytecode lowering's register model: heap values live
//! in their homes by construction, so there is nothing to flush.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_core::OpCode;
use varn_types::bytecode::decode;

/// How precisely an allocation scan reads `OpCode::Intrinsic`.
///
/// A whole FUNCTION is scanned to decide whether it needs the size gate's
/// protection, and there the cost of a false `true` is a missed compilation
/// while the cost of a false `false` is an unresumable frame — so it stays
/// conservative.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntrinsicScan {
    /// Every `Intrinsic` counts as allocating.
    Conservative,
    /// No `Intrinsic` counts: every one is a `std:math` op on scalars
    /// (`varn_core::intrinsic_ops`), which never touches the heap.
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
        if scan == IntrinsicScan::ByWireByte
            && OpCode::from_u8(code[ip] as u8) == Some(OpCode::Intrinsic)
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

/// The collector's poll at a loop back edge: when the nursery has reached its
/// threshold, `collect` runs on the slow path (it must call the
/// `gc_safepoint` helper, with whatever the lowering has to do around it).
/// A call-free allocating loop depends on it, as the interpreter's `Loop`
/// does on `gc_backedge_safepoint`: nothing else would ever collect.
pub(crate) fn emit_gc_poll(
    b: &mut FunctionBuilder,
    h: &crate::JitHelpers,
    exec_ctx: cranelift_codegen::ir::Value,
    collect: impl FnOnce(&mut FunctionBuilder),
) {
    let rcbox = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        exec_ctx,
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
    collect(b);
    b.ins().jump(cont, &[]);
    b.switch_to_block(cont);
}
