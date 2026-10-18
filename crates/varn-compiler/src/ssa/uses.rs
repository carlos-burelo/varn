use rustc_hash::FxHashMap;

use super::ir::{InstKind, SsaFunc, Terminator, Value};

macro_rules! by_value {
    ($f:ident, $v:expr) => {
        $f(*$v)
    };
}

macro_rules! by_ref {
    ($f:ident, $v:expr) => {
        $f($v)
    };
}

macro_rules! shared {
    ($c:expr) => {
        $c.iter()
    };
}

macro_rules! exclusive {
    ($c:expr) => {
        $c.iter_mut()
    };
}

macro_rules! inst_uses {
    ($kind:expr, $f:ident, $use:ident, $iter:ident) => {{
        use InstKind::*;
        match $kind {
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
            | MakeClosure { .. }
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
            | Cast { operand, .. }
            | Convert { operand, .. }
            | ToString { operand }
            | AssertNotNull { operand }
            | GetEnumTag { operand }
            | IsArray { operand }
            | StrLength { operand }
            | ArrayLength { operand }
            | BytesLength { operand }
            | ObjectKeys { operand }
            | Await { operand }
            | Spawn { operand }
            | Yield { operand } => $use!($f, operand),

            GetProperty { object, .. }
            | ObjectRest { object, .. }
            | GetFixedField { object, .. }
            | GetPropertyMaybe { object, .. }
            | ModuleSlot { object, .. }
            | GetSymbol { object, .. } => $use!($f, object),

            StoreGlobal { value, .. }
            | StoreGlobalIdx { value, .. }
            | StoreUpvalue { value, .. }
            | StoreCaptured { value, .. }
            | StoreModuleSlot { value, .. } => $use!($f, value),

            DeclareLayout { class, .. } => $use!($f, class),
            CatchParam { try_val } => $use!($f, try_val),
            MakeClass { super_class, .. } => {
                if let Some(sc) = super_class {
                    $use!($f, sc);
                }
            }

            Binary { lhs, rhs, .. } => {
                $use!($f, lhs);
                $use!($f, rhs);
            }
            GetIndex { object, index }
            | ArrayGetIndex { object, index }
            | BytesGetIndex { object, index }
            | MapGetIndex { object, index } => {
                $use!($f, object);
                $use!($f, index);
            }
            SetProperty { object, value, .. } | SetFixedField { object, value, .. } => {
                $use!($f, object);
                $use!($f, value);
            }
            ObjectMerge { target, source } => {
                $use!($f, target);
                $use!($f, source);
            }
            ArrayPush { array, value } => {
                $use!($f, array);
                $use!($f, value);
            }
            Range { start, end, .. } => {
                $use!($f, start);
                $use!($f, end);
            }
            IterCall { callee, recv } => {
                $use!($f, callee);
                $use!($f, recv);
            }
            DefineStatic { class, value, .. } => {
                $use!($f, class);
                $use!($f, value);
            }
            DefineMethod { class, method, .. } => {
                $use!($f, class);
                $use!($f, method);
            }
            DefineAccessor {
                class, accessor, ..
            } => {
                $use!($f, class);
                $use!($f, accessor);
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
            }
            | BytesSetIndex {
                object,
                index,
                value,
            }
            | MapSetIndex {
                object,
                index,
                value,
            } => {
                $use!($f, object);
                $use!($f, index);
                $use!($f, value);
            }

            SelfCall { args } | SuperCall { args } | SuperMethodCall { args, .. } => {
                $iter!(args).for_each(|v| $use!($f, v))
            }
            Call { callee, args } => {
                $use!($f, callee);
                $iter!(args).for_each(|v| $use!($f, v));
            }
            AllocInstance { class, .. } => $use!($f, class),
            MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
                $use!($f, recv);
                $iter!(args).for_each(|v| $use!($f, v));
            }
            IntrinsicCall { object, args, .. } | CallNativeOp { object, args, .. } => {
                $use!($f, object);
                $iter!(args).for_each(|v| $use!($f, v));
            }
            BuildArray { elements, .. }
            | BuildTuple { elements }
            | BuildStr { parts: elements } => $iter!(elements).for_each(|v| $use!($f, v)),
            BuildObject { pairs } | BuildRecord { pairs } => {
                $iter!(pairs).for_each(|(_, v)| $use!($f, v))
            }
            BuildMap { pairs } => $iter!(pairs).for_each(|(k, v)| {
                $use!($f, k);
                $use!($f, v);
            }),
            CallSpread { callee, args } => {
                $use!($f, callee);
                $iter!(args).for_each(|(a, _)| $use!($f, a));
            }
            BuildArraySpread { elements } => $iter!(elements).for_each(|(e, _)| $use!($f, e)),
            BuildObjectSpread { parts } => $iter!(parts).for_each(|(_, v)| $use!($f, v)),
        }
    }};
}

macro_rules! term_uses {
    ($term:expr, $f:ident, $use:ident, $iter:ident) => {
        match $term {
            Terminator::Return(Some(v)) | Terminator::Throw(v) => $use!($f, v),
            Terminator::Return(None) | Terminator::Unreachable => {}
            Terminator::Jump { args, .. } => {
                for a in $iter!(args) {
                    $use!($f, a);
                }
            }
            Terminator::Branch {
                cond,
                then_args,
                else_args,
                ..
            } => {
                $use!($f, cond);
                for a in $iter!(then_args) {
                    $use!($f, a);
                }
                for a in $iter!(else_args) {
                    $use!($f, a);
                }
            }
        }
    };
}

pub fn visit_uses(kind: &InstKind, f: &mut impl FnMut(Value)) {
    inst_uses!(kind, f, by_value, shared)
}

pub fn visit_uses_mut(kind: &mut InstKind, f: &mut impl FnMut(&mut Value)) {
    inst_uses!(kind, f, by_ref, exclusive)
}

pub fn visit_term_uses(term: &Terminator, f: &mut impl FnMut(Value)) {
    term_uses!(term, f, by_value, shared)
}

pub fn visit_term_uses_mut(term: &mut Terminator, mut f: impl FnMut(&mut Value)) {
    term_uses!(term, f, by_ref, exclusive)
}

pub fn replace_uses_with_map(func: &mut SsaFunc, map: &FxHashMap<Value, Value>) -> bool {
    if map.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut sub = |v: &mut Value| {
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
