













use rustc_hash::FxHashMap;

use super::ir::{InstKind, SsaFunc, Terminator, Value};


pub fn visit_uses(kind: &InstKind, f: &mut impl FnMut(Value)) {
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
        | Yield { operand } => f(*operand),

        GetProperty { object, .. }
        | ObjectRest { object, .. }
        | GetFixedField { object, .. }
        | GetPropertyMaybe { object, .. }
        | ModuleSlot { object, .. }
        | GetSymbol { object, .. } => f(*object),

        StoreGlobal { value, .. }
        | StoreGlobalIdx { value, .. }
        | StoreUpvalue { value, .. }
        | StoreCaptured { value, .. }
        | StoreModuleSlot { value, .. } => f(*value),

        DeclareLayout { class, .. } => f(*class),
        CatchParam { try_val } => f(*try_val),
        MakeClass { super_class, .. } => {
            if let Some(sc) = super_class {
                f(*sc);
            }
        }

        
        Binary { lhs, rhs, .. } => {
            f(*lhs);
            f(*rhs);
        }
        GetIndex { object, index }
        | ArrayGetIndex { object, index }
        | MapGetIndex { object, index } => {
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
        ArrayPush { array, value } => {
            f(*array);
            f(*value);
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
        | MapSetIndex {
            object,
            index,
            value,
        } => {
            f(*object);
            f(*index);
            f(*value);
        }

        
        SelfCall { args } | SuperCall { args } | SuperMethodCall { args, .. } => {
            args.iter().for_each(|a| f(*a))
        }
        Call { callee, args } => {
            f(*callee);
            args.iter().for_each(|a| f(*a));
        }
        AllocInstance { class } => f(*class),
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(*recv);
            args.iter().for_each(|a| f(*a));
        }
        IntrinsicCall { object, args, .. } | CallNativeOp { object, args, .. } => {
            f(*object);
            args.iter().for_each(|a| f(*a));
        }
        BuildArray { elements, .. } | BuildTuple { elements } | BuildStr { parts: elements } => {
            elements.iter().for_each(|e| f(*e))
        }
        BuildObject { pairs } | BuildRecord { pairs } => pairs.iter().for_each(|(_, v)| f(*v)),
        BuildMap { pairs } => pairs.iter().for_each(|(k, v)| {
            f(*k);
            f(*v);
        }),
        CallSpread { callee, args } => {
            f(*callee);
            args.iter().for_each(|(a, _)| f(*a));
        }
        BuildArraySpread { elements } => elements.iter().for_each(|(e, _)| f(*e)),
        BuildObjectSpread { parts } => parts.iter().for_each(|(_, v)| f(*v)),
    }
}


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
        | Yield { operand } => f(operand),

        GetProperty { object, .. }
        | ObjectRest { object, .. }
        | GetFixedField { object, .. }
        | GetPropertyMaybe { object, .. }
        | ModuleSlot { object, .. }
        | GetSymbol { object, .. } => f(object),

        StoreGlobal { value, .. }
        | StoreGlobalIdx { value, .. }
        | StoreUpvalue { value, .. }
        | StoreCaptured { value, .. }
        | StoreModuleSlot { value, .. } => f(value),

        DeclareLayout { class, .. } => f(class),
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
        GetIndex { object, index }
        | ArrayGetIndex { object, index }
        | MapGetIndex { object, index } => {
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
        ArrayPush { array, value } => {
            f(array);
            f(value);
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
        }
        | MapSetIndex {
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
        AllocInstance { class } => f(class),
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(recv);
            args.iter_mut().for_each(f);
        }
        IntrinsicCall { object, args, .. } | CallNativeOp { object, args, .. } => {
            f(object);
            args.iter_mut().for_each(f);
        }
        BuildArray { elements, .. } | BuildTuple { elements } | BuildStr { parts: elements } => {
            elements.iter_mut().for_each(f)
        }
        BuildObject { pairs } | BuildRecord { pairs } => pairs.iter_mut().for_each(|(_, v)| f(v)),
        BuildMap { pairs } => pairs.iter_mut().for_each(|(k, v)| {
            f(k);
            f(v);
        }),
        CallSpread { callee, args } => {
            f(callee);
            args.iter_mut().for_each(|(a, _)| f(a));
        }
        BuildArraySpread { elements } => elements.iter_mut().for_each(|(e, _)| f(e)),
        BuildObjectSpread { parts } => parts.iter_mut().for_each(|(_, v)| f(v)),
    }
}


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


pub fn visit_term_uses_mut(term: &mut Terminator, mut f: impl FnMut(&mut Value)) {
    match term {
        Terminator::Return(Some(v)) | Terminator::Throw(v) => f(v),
        Terminator::Return(None) | Terminator::Unreachable => {}
        Terminator::Jump { args, .. } => {
            for a in args {
                f(a);
            }
        }
        Terminator::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            f(cond);
            for a in then_args {
                f(a);
            }
            for a in else_args {
                f(a);
            }
        }
    }
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
