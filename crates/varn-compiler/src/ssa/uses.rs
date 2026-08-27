//! The one place that knows which fields of an [`InstKind`] are operands.
//!
//! There used to be three hand-written copies of this traversal — the
//! substitution in `SsaFunc::replace_all_uses`, the collection in
//! `verify::inst_uses`, and a partial one in `passes::licm::operands` that
//! silently returned "no operands" for anything it had not been taught. A new
//! `InstKind` had to be added to all three, and the third failing open meant
//! a pass could hoist an instruction past the definition of an operand it did
//! not know existed.
//!
//! Both visitors below are **exhaustive matches with no wildcard arm**, so a
//! new variant stops the build until it is classified. Everything else in the
//! crate is expressed on top of them.

use rustc_hash::FxHashMap;

use super::ir::{InstKind, SsaFunc, Terminator, Value};

/// Calls `f` on every value `kind` reads, in operand order.
pub fn visit_uses(kind: &InstKind, f: &mut impl FnMut(Value)) {
    use InstKind::*;
    match kind {
        // No operands.
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
        | MakeEnumVariant { .. }
        | Try { .. }
        | PopTry
        | CloseUpvalues { .. }
        | Dispose { .. }
        | LoadModule { .. }
        | This
        | GetSuper { .. } => {}

        // One operand.
        Unary { operand, .. }
        | IsNull { operand }
        | ToString { operand }
        | AssertNotNull { operand }
        | GetEnumTag { operand }
        | IsArray { operand }
        | ObjectKeys { operand }
        | Await { operand }
        | Spawn { operand }
        | Yield { operand } => f(*operand),

        GetProperty { object, .. }
        | ObjectRest { object, .. }
        | GetFixedField { object, .. }
        | GetPropertyMaybe { object, .. }
        | ModuleSlot { object, .. }
        | GetSymbol { object, .. } => f(*object),

        StoreGlobal { value, .. }
        | StoreUpvalue { value, .. }
        | StoreCaptured { value, .. }
        | StoreModuleSlot { value, .. } => f(*value),

        DeclareField { class, .. } => f(*class),
        CatchParam { try_val } => f(*try_val),
        MakeClass { super_class, .. } => {
            if let Some(sc) = super_class {
                f(*sc);
            }
        }

        // Two operands.
        Binary { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }
        GetIndex { object, index } | ArrayGetIndex { object, index } => {
            f(*object);
            f(*index);
        }
        SetProperty { object, value, .. } | SetFixedField { object, value, .. } => {
            f(*object);
            f(*value);
        }
        ObjectMerge { target, source } => {
            f(*target);
            f(*source);
        }
        Range { start, end, .. } => {
            f(*start);
            f(*end);
        }
        IterCall { callee, recv } => {
            f(*callee);
            f(*recv);
        }
        DefineStatic { class, value, .. } => {
            f(*class);
            f(*value);
        }
        DefineMethod { class, method, .. } => {
            f(*class);
            f(*method);
        }
        DefineAccessor {
            class, accessor, ..
        } => {
            f(*class);
            f(*accessor);
        }

        // Three operands.
        SetIndex {
            object,
            index,
            value,
        }
        | ArraySetIndex {
            object,
            index,
            value,
        } => {
            f(*object);
            f(*index);
            f(*value);
        }

        // Variadic.
        SelfCall { args } | SuperCall { args } | SuperMethodCall { args, .. } => {
            args.iter().for_each(|a| f(*a))
        }
        Call { callee, args } => {
            f(*callee);
            args.iter().for_each(|a| f(*a));
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(*recv);
            args.iter().for_each(|a| f(*a));
        }
        IntrinsicCall { object, args, .. } | CallNativeOp { object, args, .. } => {
            f(*object);
            args.iter().for_each(|a| f(*a));
        }
        BuildArray { elements } | BuildTuple { elements } | BuildStr { parts: elements } => {
            elements.iter().for_each(|e| f(*e))
        }
        MakeClosure { upvalues, .. } => upvalues.iter().for_each(|u| f(*u)),
        BuildObject { pairs } | BuildRecord { pairs } => pairs.iter().for_each(|(_, v)| f(*v)),
        CallSpread { callee, args } => {
            f(*callee);
            args.iter().for_each(|(a, _)| f(*a));
        }
        BuildArraySpread { elements } => elements.iter().for_each(|(e, _)| f(*e)),
        BuildObjectSpread { parts } => parts.iter().for_each(|(_, v)| f(*v)),
    }
}

/// Mutable twin of [`visit_uses`], for substitution.
pub fn visit_uses_mut(kind: &mut InstKind, f: &mut impl FnMut(&mut Value)) {
    use InstKind::*;
    match kind {
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
        | MakeEnumVariant { .. }
        | Try { .. }
        | PopTry
        | CloseUpvalues { .. }
        | Dispose { .. }
        | LoadModule { .. }
        | This
        | GetSuper { .. } => {}

        Unary { operand, .. }
        | IsNull { operand }
        | ToString { operand }
        | AssertNotNull { operand }
        | GetEnumTag { operand }
        | IsArray { operand }
        | ObjectKeys { operand }
        | Await { operand }
        | Spawn { operand }
        | Yield { operand } => f(operand),

        GetProperty { object, .. }
        | ObjectRest { object, .. }
        | GetFixedField { object, .. }
        | GetPropertyMaybe { object, .. }
        | ModuleSlot { object, .. }
        | GetSymbol { object, .. } => f(object),

        StoreGlobal { value, .. }
        | StoreUpvalue { value, .. }
        | StoreCaptured { value, .. }
        | StoreModuleSlot { value, .. } => f(value),

        DeclareField { class, .. } => f(class),
        CatchParam { try_val } => f(try_val),
        MakeClass { super_class, .. } => {
            if let Some(sc) = super_class {
                f(sc);
            }
        }

        Binary { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        GetIndex { object, index } | ArrayGetIndex { object, index } => {
            f(object);
            f(index);
        }
        SetProperty { object, value, .. } | SetFixedField { object, value, .. } => {
            f(object);
            f(value);
        }
        ObjectMerge { target, source } => {
            f(target);
            f(source);
        }
        Range { start, end, .. } => {
            f(start);
            f(end);
        }
        IterCall { callee, recv } => {
            f(callee);
            f(recv);
        }
        DefineStatic { class, value, .. } => {
            f(class);
            f(value);
        }
        DefineMethod { class, method, .. } => {
            f(class);
            f(method);
        }
        DefineAccessor {
            class, accessor, ..
        } => {
            f(class);
            f(accessor);
        }

        SetIndex {
            object,
            index,
            value,
        }
        | ArraySetIndex {
            object,
            index,
            value,
        } => {
            f(object);
            f(index);
            f(value);
        }

        SelfCall { args } | SuperCall { args } | SuperMethodCall { args, .. } => {
            args.iter_mut().for_each(f)
        }
        Call { callee, args } => {
            f(callee);
            args.iter_mut().for_each(f);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(recv);
            args.iter_mut().for_each(f);
        }
        IntrinsicCall { object, args, .. } | CallNativeOp { object, args, .. } => {
            f(object);
            args.iter_mut().for_each(f);
        }
        BuildArray { elements } | BuildTuple { elements } | BuildStr { parts: elements } => {
            elements.iter_mut().for_each(f)
        }
        MakeClosure { upvalues, .. } => upvalues.iter_mut().for_each(f),
        BuildObject { pairs } | BuildRecord { pairs } => pairs.iter_mut().for_each(|(_, v)| f(v)),
        CallSpread { callee, args } => {
            f(callee);
            args.iter_mut().for_each(|(a, _)| f(a));
        }
        BuildArraySpread { elements } => elements.iter_mut().for_each(|(e, _)| f(e)),
        BuildObjectSpread { parts } => parts.iter_mut().for_each(|(_, v)| f(v)),
    }
}

/// Calls `f` on every value a terminator reads.
pub fn visit_term_uses(term: &Terminator, f: &mut impl FnMut(Value)) {
    match term {
        Terminator::Return(Some(v)) | Terminator::Throw(v) => f(*v),
        Terminator::Return(None) | Terminator::Unreachable => {}
        Terminator::Jump { args, .. } => args.iter().for_each(|a| f(*a)),
        Terminator::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            f(*cond);
            then_args.iter().for_each(|a| f(*a));
            else_args.iter().for_each(|a| f(*a));
        }
    }
}

/// Mutable twin of [`visit_term_uses`].
pub fn visit_term_uses_mut(term: &mut Terminator, f: &mut impl FnMut(&mut Value)) {
    match term {
        Terminator::Return(Some(v)) | Terminator::Throw(v) => f(v),
        Terminator::Return(None) | Terminator::Unreachable => {}
        Terminator::Jump { args, .. } => args.iter_mut().for_each(|a| f(a)),
        Terminator::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            f(cond);
            then_args.iter_mut().for_each(|a| f(a));
            else_args.iter_mut().for_each(|a| f(a));
        }
    }
}

/// Where a value is defined. Block params have no defining instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Def {
    /// `blocks[block].insts[index]`.
    Inst { block: u32, index: u32 },
    /// `blocks[block].params[index]`.
    Param { block: u32, index: u32 },
}

/// Def site of every value id, indexed by `Value.0`.
///
/// `None` means the id has no reaching definition in the current shape of the
/// function — either it was never materialized or its defining instruction has
/// already been deleted. Callers must treat that as "unknown", never as a
/// value they may move or fold.
pub fn def_sites(func: &SsaFunc) -> Vec<Option<Def>> {
    let mut defs = vec![None; func.values.len()];
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, &p) in block.params.iter().enumerate() {
            if let Some(slot) = defs.get_mut(p.0 as usize) {
                *slot = Some(Def::Param {
                    block: b as u32,
                    index: i as u32,
                });
            }
        }
        for (i, inst) in block.insts.iter().enumerate() {
            if let Some(d) = inst.dest {
                if let Some(slot) = defs.get_mut(d.0 as usize) {
                    *slot = Some(Def::Inst {
                        block: b as u32,
                        index: i as u32,
                    });
                }
            }
        }
    }
    defs
}

/// Applies many substitutions in a single traversal.
///
/// The per-value [`SsaFunc::replace_all_uses`] walks every instruction in the
/// function, so a pass with `n` rewrites pays `n` full passes. Rewrites are
/// chased transitively, so a map containing both `a -> b` and `b -> c`
/// resolves uses of `a` to `c` regardless of insertion order.
pub fn replace_uses_with_map(func: &mut SsaFunc, map: &FxHashMap<Value, Value>) -> bool {
    if map.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut sub = |v: &mut Value| {
        // Bounded by the map size: each hop consumes a distinct entry, and
        // `insert_rewrite` refuses to create a cycle.
        let mut hops = 0;
        while let Some(&next) = map.get(v) {
            if next == *v || hops > map.len() {
                break;
            }
            *v = next;
            changed = true;
            hops += 1;
        }
    };
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            visit_uses_mut(&mut inst.kind, &mut sub);
        }
        visit_term_uses_mut(&mut block.term, &mut sub);
    }
    changed
}
