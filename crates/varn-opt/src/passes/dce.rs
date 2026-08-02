use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator, Value};
use rustc_hash::FxHashSet;

pub fn run(func: &mut SsaFunc) -> bool {
    let mut changed = false;
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
        for inst in old_insts {
            if let Some(dest) = inst.dest {
                if !used.contains(&dest) && is_pure(&inst.kind) {
                    changed = true;
                    continue;
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
/// integer exponent all raise, so those three operators stay.
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
        | LoadUpvalue(_)
        | LoadCaptured { .. }
        | ModuleSlot { .. }
        | This
        | CatchParam { .. } => true,

        // Fixed slots and statically-typed array elements are plain memory:
        // the checker proved the receiver's shape, so no accessor can run.
        GetFixedField { .. } | ArrayGetIndex { .. } => true,

        // Type tests and tag reads inspect the value, never dispatch.
        IsNull { .. } | IsArray { .. } | GetEnumTag { .. } | ObjectKeys { .. } => true,

        // Allocation with no observable effect.
        BuildArray { .. }
        | BuildObject { .. }
        | MakeClosure { .. }
        | MakeEnumVariant { .. }
        | Range { .. } => true,

        Binary { op, ty, .. } => {
            let typed = matches!(ty, HirType::Int | HirType::Float | HirType::Bool);
            // Div/Mod/Pow raise on zero divisor and negative exponent.
            let total = matches!(
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
            typed && total
        }
        Unary { op, ty, .. } => match op {
            HirUnOp::Typeof => true,
            HirUnOp::Neg | HirUnOp::Not | HirUnOp::BitNot => {
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
        BuildArraySpread { .. } | BuildObjectSpread { .. } | CallSpread { .. } => false,

        // Calls, in every shape.
        Call { .. }
        | SelfCall { .. }
        | MethodCall { .. }
        | SuperCall { .. }
        | SuperMethodCall { .. }
        | ExtensionCall { .. }
        | IterCall { .. }
        | IntrinsicCall { .. }
        | CallNativeOp { .. } => false,

        // Stores and other observable writes.
        StoreGlobal { .. }
        | StoreUpvalue { .. }
        | StoreCaptured { .. }
        | StoreModuleSlot { .. }
        | SetProperty { .. }
        | SetFixedField { .. }
        | SetIndex { .. }
        | ArraySetIndex { .. }
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

fn remove_param(func: &mut SsaFunc, block: BlockId, pos: usize) {
    func.blocks[block.0 as usize].params.remove(pos);
    let preds = func.blocks[block.0 as usize].preds.clone();
    for pred in preds {
        match &mut func.blocks[pred.0 as usize].term {
            Terminator::Jump { target, args } if *target == block => {
                args.remove(pos);
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    then_args.remove(pos);
                }
                if *else_blk == block {
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
