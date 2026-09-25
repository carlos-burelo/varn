use super::cfg::{dominates, dominators};
use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;

    changed |= eliminate_trivial_phis(func);

    let mut used = FxHashSet::default();

    for block in &func.blocks {
        for inst in &block.insts {
            add_inst_uses(&inst.kind, &mut used);
        }
        add_term_uses(&block.term, &mut used);
    }

    for block_idx in 0..func.blocks.len() {
        let b_id = BlockId(block_idx as u32);
        let mut new_insts = Vec::new();

        let old_insts = std::mem::take(&mut func.blocks[b_id.0 as usize].insts);
        for mut inst in old_insts {
            if let Some(dest) = inst.dest {
                if !used.contains(&dest) {
                    if is_pure(&inst.kind) {
                        changed = true;
                        continue;
                    }
                    if dest_droppable(&inst.kind) {
                        inst.dest = None;
                        changed = true;
                    }
                }
            }
            new_insts.push(inst);
        }
        func.blocks[b_id.0 as usize].insts = new_insts;
    }

    for b_idx in 0..func.blocks.len() {
        let b_id = BlockId(b_idx as u32);
        if b_id == func.entry {
            continue;
        }

        let mut pos = 0;
        while pos < func.blocks[b_id.0 as usize].params.len() {
            let phi = func.blocks[b_id.0 as usize].params[pos];
            if !used.contains(&phi) {
                remove_param(func, b_id, pos);
                changed = true;
            } else {
                pos += 1;
            }
        }
    }

    changed
}

/// Whether an instruction that must RUN can still give up its destination
/// when nothing reads it.
///
/// A call to a `void` function is the common case, and in a test corpus it is
/// most of the code: `assert(...)`, `print(...)`. The call has to happen, but
/// its result is nobody's — and while it kept a destination it kept an SSA
/// value, a live range, a register the allocator had to colour and, since
/// `void` has no value type, a `Dynamic` one at that.
///
/// Restricted to the call family on purpose. Everything else that defines a
/// value either has its destination read (or the pass above would have
/// deleted it) or uses it as part of a protocol the emitter depends on —
/// `Try`'s landing value, `CatchParam`, the suspension points. A `false` here
/// costs a register; a wrong `true` loses an instruction, because
/// `emit_inst` hands a destination-less instruction to `emit_effect` and
/// drops whatever that does not recognize.
pub(crate) fn dest_droppable(kind: &InstKind) -> bool {
    use InstKind::*;
    matches!(
        kind,
        Call { .. }
            | SelfCall { .. }
            | MethodCall { .. }
            | SuperCall { .. }
            | SuperMethodCall { .. }
            | ExtensionCall { .. }
            | IntrinsicCall { .. }
            | CallNativeOp { .. }
    )
}

/// Whether an instruction can be deleted when its result is unused.
///
/// Deliberately an **allow-list, written as an exhaustive match**: a new
/// `InstKind` must be classified by hand or this stops compiling. The
/// previous deny-list had the opposite default, so anything it forgot was
/// silently deletable — that is how `GetProperty` on a side-effecting getter
/// came to be dropped.
///
/// "Pure" here means: runs no user code, writes nothing observable, and
/// cannot throw. Allocation alone is fine — an unobserved allocation is
/// exactly what we want gone. Trap behaviour was measured, not assumed: an
/// out-of-bounds array read yields `null`, while `/ 0`, `% 0` and a negative
/// integer exponent all raise, and `int` `+ - *` and `-x` overflow, so they stay.
pub(crate) fn is_pure(kind: &InstKind) -> bool {
    use InstKind::*;
    match kind {
        // Values and plain memory reads.
        ConstInt(_)
        | ConstFloat(_)
        | ConstBool(_)
        | ConstStr(_)
        | ConstChar(_)
        | ConstDecimal(_)
        | ConstBigInt(_)
        | ConstNull
        | LoadGlobal(_)
        | LoadGlobalIdx(_)
        | LoadNativeGlobalIdx(_)
        | LoadUpvalue(_)
        | LoadCaptured { .. }
        | ModuleSlot { .. }
        | This
        | CatchParam { .. } => true,

        // Fixed slots and statically-typed array elements are plain memory:
        // the checker proved the receiver's shape, so no accessor can run.
        GetFixedField { .. } | ArrayGetIndex { .. } | MapGetIndex { .. } => true,

        // Type tests and tag reads inspect the value, never dispatch.
        IsNull { .. } | Cast { .. } | IsArray { .. } | GetEnumTag { .. } | ObjectKeys { .. } => {
            true
        }
        // A length of a receiver statically typed `str` / array: a read, no
        // getter can run.
        StrLength { .. } | ArrayLength { .. } => true,
        Convert { conv, .. } => !conv.can_fault(),

        // Puede lanzar si el valor no cabe en el ancho declarado (mismo
        // motivo que Div/Mod/Pow abajo: el panic ES el efecto observable,
        // incluso con el resultado descartado).

        // Allocation with no observable effect.
        BuildArray { .. }
        | BuildTuple { .. }
        | BuildObject { .. }
        | BuildRecord { .. }
        | BuildMap { .. }
        | MakeClosure { .. }
        | MakeEnumVariant { .. }
        | Range { .. } => true,

        Binary { op, ty, .. } => {
            let typed = matches!(ty, HirType::Int | HirType::Float | HirType::Bool);
            let int_can_overflow =
                *ty == HirType::Int && matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul);
            // Div/Mod/Pow raise on zero divisor and negative exponent.
            let never_traps = matches!(
                op,
                HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::Eq
                    | HirBinOp::Ne
                    | HirBinOp::Lt
                    | HirBinOp::Le
                    | HirBinOp::Gt
                    | HirBinOp::Ge
                    | HirBinOp::BitAnd
                    | HirBinOp::BitOr
                    | HirBinOp::BitXor
                    | HirBinOp::Shl
                    | HirBinOp::Shr
                    | HirBinOp::Ushr
            );
            typed && never_traps && !int_can_overflow
        }
        Unary { op, ty, .. } => match op {
            HirUnOp::Typeof => true,
            HirUnOp::Neg => matches!(ty, HirType::Float),
            HirUnOp::Not | HirUnOp::BitNot => {
                matches!(ty, HirType::Int | HirType::Float | HirType::Bool)
            }
        },

        // `GetProperty` runs a getter when the class declares one — the
        // regression this list exists to prevent. The other property-shaped
        // reads do not reach accessors today, but they resolve names through
        // the same runtime path, so they are classified with it rather than
        // on an incidental current behaviour.
        GetProperty { .. }
        | GetPropertyMaybe { .. }
        | GetIndex { .. }
        | GetSuper { .. }
        | GetSymbol { .. } => false,

        // Stringifying a class instance yields `[object Object]` today — no
        // user `toString` dispatch — but that is the contract most likely to
        // grow one, and dead interpolations are too rare for the distinction
        // to buy anything.
        ToString { .. } | BuildStr { .. } => false,

        // Spread iterates the operand, which runs its iterator.
        BuildArraySpread { .. }
        | BuildObjectSpread { .. }
        | CallSpread { .. }
        | ObjectRest { .. } => false,

        // Calls, in every shape.
        Call { .. }
        | SelfCall { .. }
        | MethodCall { .. }
        | SuperCall { .. }
        | SuperMethodCall { .. }
        | ExtensionCall { .. }
        | IterCall { .. }
        | IntrinsicCall { .. }
        | CallNativeOp { .. }
        | ArrayPush { .. } => false,

        // Stores and other observable writes.
        StoreGlobal { .. }
        | StoreGlobalIdx { .. }
        | StoreUpvalue { .. }
        | StoreCaptured { .. }
        | StoreModuleSlot { .. }
        | SetProperty { .. }
        | SetFixedField { .. }
        | SetIndex { .. }
        | ArraySetIndex { .. }
        | MapSetIndex { .. }
        | ObjectMerge { .. } => false,

        // Class construction mutates the class object being built.
        MakeClass { .. }
        | DeclareField { .. }
        | DefineStatic { .. }
        | DefineMethod { .. }
        | DefineAccessor { .. } => false,

        // Control-flow and runtime state.
        Try { .. }
        | PopTry
        | CloseUpvalues { .. }
        | Dispose { .. }
        | LoadModule { .. }
        | AssertNotNull { .. }
        | Await { .. }
        | Spawn { .. }
        | Yield { .. } => false,
    }
}

pub(crate) fn eliminate_trivial_phis(func: &mut SsaFunc) -> bool {
    let n = func.blocks.len();
    if n < 2 {
        return false;
    }

    crate::ssa::verify::recompute_preds(func);

    let mut def_block = vec![None; func.values.len()];
    for (b_idx, block) in func.blocks.iter().enumerate() {
        let bid = BlockId(b_idx as u32);
        for &p in &block.params {
            def_block[p.0 as usize] = Some(bid);
        }
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                def_block[d.0 as usize] = Some(bid);
            }
        }
    }

    let dom = dominators(func);
    let mut any_changed = false;

    loop {
        let mut changed = false;

        for b_idx in 0..func.blocks.len() {
            let b_id = BlockId(b_idx as u32);
            if b_id == func.entry {
                continue;
            }

            let mut pos = 0;
            while pos < func.blocks[b_id.0 as usize].params.len() {
                let phi = func.blocks[b_id.0 as usize].params[pos];

                let Some(incoming) = get_incoming_args(func, b_id, pos) else {
                    pos += 1;
                    continue;
                };

                if incoming.is_empty() {
                    pos += 1;
                    continue;
                }

                let mut same: Option<Value> = None;
                let mut is_trivial = true;

                for &arg in &incoming {
                    if arg == phi {
                        continue;
                    }
                    match same {
                        None => same = Some(arg),
                        Some(v) if v == arg => {}
                        _ => {
                            is_trivial = false;
                            break;
                        }
                    }
                }

                if !is_trivial {
                    pos += 1;
                    continue;
                }

                let Some(unique_val) = same else {
                    pos += 1;
                    continue;
                };

                if func.value_ty(unique_val) != func.value_ty(phi) {
                    pos += 1;
                    continue;
                }

                let Some(def_b) = def_block[unique_val.0 as usize] else {
                    pos += 1;
                    continue;
                };

                let can_replace = if def_b == b_id {
                    unique_val != phi && func.blocks[b_id.0 as usize].params.contains(&unique_val)
                } else {
                    dominates(&dom, def_b.0 as usize, b_id.0 as usize)
                };

                if !can_replace {
                    pos += 1;
                    continue;
                }

                func.replace_all_uses(phi, unique_val);
                remove_param(func, b_id, pos);
                changed = true;
                any_changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    any_changed
}

fn get_incoming_args(func: &SsaFunc, block: BlockId, pos: usize) -> Option<Vec<Value>> {
    let mut args = Vec::new();
    let mut preds = func.blocks[block.0 as usize].preds.clone();
    preds.sort_unstable_by_key(|p| p.0);
    preds.dedup();
    for pred in preds {
        match &func.blocks[pred.0 as usize].term {
            Terminator::Jump {
                target,
                args: j_args,
            } => {
                if *target == block {
                    if let Some(&arg) = j_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    if let Some(&arg) = then_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
                if *else_blk == block {
                    if let Some(&arg) = else_args.get(pos) {
                        args.push(arg);
                    } else {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    Some(args)
}

fn remove_param(func: &mut SsaFunc, block: BlockId, pos: usize) {
    func.blocks[block.0 as usize].params.remove(pos);
    let mut preds = func.blocks[block.0 as usize].preds.clone();
    preds.sort_unstable_by_key(|p| p.0);
    preds.dedup();
    for pred in preds {
        match &mut func.blocks[pred.0 as usize].term {
            Terminator::Jump { target, args } if *target == block => {
                if pos < args.len() {
                    args.remove(pos);
                }
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block && pos < then_args.len() {
                    then_args.remove(pos);
                }
                if *else_blk == block && pos < else_args.len() {
                    else_args.remove(pos);
                }
            }
            _ => {}
        }
    }
}

fn add_inst_uses(kind: &InstKind, used: &mut FxHashSet<Value>) {
    used.extend(crate::ssa::verify::inst_uses(kind));
}

fn add_term_uses(term: &Terminator, used: &mut FxHashSet<Value>) {
    match term {
        Terminator::Return(Some(v)) => {
            used.insert(*v);
        }
        Terminator::Throw(v) => {
            used.insert(*v);
        }
        Terminator::Jump { args, .. } => {
            for &arg in args {
                used.insert(arg);
            }
        }
        Terminator::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            used.insert(*cond);
            for &arg in then_args {
                used.insert(arg);
            }
            for &arg in else_args {
                used.insert(arg);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "dce_purity_tests.rs"]
mod purity_tests;
