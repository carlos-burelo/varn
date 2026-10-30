use std::ffi::{CStr, CString};
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

#[derive(Clone, Copy)]
enum Kind {
    Void,
    Int,
    Float,
    Str,
    Ptr,
    Bool,
}

impl Kind {
    fn from_raw(raw: i64) -> Option<Kind> {
        match raw {
            0 => Some(Kind::Void),
            1 => Some(Kind::Int),
            2 => Some(Kind::Float),
            3 => Some(Kind::Str),
            4 => Some(Kind::Ptr),
            5 => Some(Kind::Bool),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Void => "void",
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Str => "str",
            Kind::Ptr => "ptr",
            Kind::Bool => "bool",
        }
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Int(u64),
    Float(f64),
}

impl Slot {
    #[cfg(windows)]
    fn bits(self) -> u64 {
        match self {
            Slot::Int(bits) => bits,
            Slot::Float(value) => value.to_bits(),
        }
    }
}

fn read_kinds(ctx: &mut dyn NativeCtx, types: &VnArray) -> Result<Vec<Kind>, NativeError> {
    (0..types.len(ctx))
        .map(|i| {
            types
                .get(ctx, i)
                .filter(|v| v.is_int())
                .and_then(|v| Kind::from_raw(v.as_int()))
                .ok_or_else(|| NativeError::type_error(format!("ffi.call: parameter {i} has an unknown type")))
        })
        .collect()
}

fn marshal(
    ctx: &mut dyn NativeCtx,
    kinds: &[Kind],
    values: &[VmValue],
    keep: &mut Vec<CString>,
) -> Result<Vec<Slot>, NativeError> {
    let mut slots = Vec::with_capacity(kinds.len());
    for (i, (kind, v)) in kinds.iter().zip(values).enumerate() {
        let mismatch = || NativeError::type_error(format!("ffi.call: argument {i} must be {}", kind.label()));
        let slot = match kind {
            Kind::Void => return Err(NativeError::type_error(format!("ffi.call: parameter {i} cannot be void"))),
            Kind::Int => {
                if !v.is_int() {
                    return Err(mismatch());
                }
                Slot::Int(v.as_int() as u64)
            }
            Kind::Float => {
                if !v.is_f64() {
                    return Err(mismatch());
                }
                Slot::Float(v.as_f64())
            }
            Kind::Bool => {
                if !v.is_bool() {
                    return Err(mismatch());
                }
                Slot::Int(u64::from(v.as_bool()))
            }
            Kind::Ptr => {
                if v.is_null() {
                    Slot::Int(0)
                } else if v.is_int() {
                    Slot::Int(v.as_int() as u64)
                } else {
                    return Err(mismatch());
                }
            }
            Kind::Str => {
                if v.is_null() {
                    Slot::Int(0)
                } else {
                    let text = ctx.str_owned(*v).ok_or_else(mismatch)?;
                    let c_text = CString::new(text)
                        .map_err(|_| NativeError::type_error(format!("ffi.call: argument {i} contains a NUL byte")))?;
                    let addr = c_text.as_ptr() as usize as u64;
                    keep.push(c_text);
                    Slot::Int(addr)
                }
            }
        };
        slots.push(slot);
    }
    Ok(slots)
}

#[cfg(windows)]
mod abi {
    use super::Slot;
    use varn_types::NativeError;

    pub const MAX_ARGS: usize = 8;

    trait Reg: Copy {
        fn from_bits(bits: u64) -> Self;
    }

    impl Reg for usize {
        fn from_bits(bits: u64) -> Self {
            bits as usize
        }
    }

    impl Reg for f64 {
        fn from_bits(bits: u64) -> Self {
            f64::from_bits(bits)
        }
    }

    unsafe fn call4<A: Reg, B: Reg, C: Reg, D: Reg, R>(f: usize, b: &[u64; MAX_ARGS]) -> R {
        let f = unsafe {
            std::mem::transmute::<
                usize,
                extern "C" fn(A, B, C, D, usize, usize, usize, usize) -> R,
            >(f)
        };
        f(
            A::from_bits(b[0]),
            B::from_bits(b[1]),
            C::from_bits(b[2]),
            D::from_bits(b[3]),
            b[4] as usize,
            b[5] as usize,
            b[6] as usize,
            b[7] as usize,
        )
    }

    pub fn dispatch<R>(f: usize, slots: &[Slot]) -> Result<R, NativeError> {
        if slots.len() > MAX_ARGS {
            return Err(NativeError::range_error(format!(
                "ffi.call: at most {MAX_ARGS} arguments are supported, got {}",
                slots.len()
            )));
        }
        let mut bits = [0u64; MAX_ARGS];
        let mut float = [false; 4];
        for (i, slot) in slots.iter().enumerate() {
            bits[i] = slot.bits();
            if let (Some(flag), Slot::Float(_)) = (float.get_mut(i), slot) {
                *flag = true;
            }
        }
        type I = usize;
        type F = f64;
        let b = &bits;
        Ok(unsafe {
            match (float[0], float[1], float[2], float[3]) {
                (false, false, false, false) => call4::<I, I, I, I, R>(f, b),
                (false, false, false, true) => call4::<I, I, I, F, R>(f, b),
                (false, false, true, false) => call4::<I, I, F, I, R>(f, b),
                (false, false, true, true) => call4::<I, I, F, F, R>(f, b),
                (false, true, false, false) => call4::<I, F, I, I, R>(f, b),
                (false, true, false, true) => call4::<I, F, I, F, R>(f, b),
                (false, true, true, false) => call4::<I, F, F, I, R>(f, b),
                (false, true, true, true) => call4::<I, F, F, F, R>(f, b),
                (true, false, false, false) => call4::<F, I, I, I, R>(f, b),
                (true, false, false, true) => call4::<F, I, I, F, R>(f, b),
                (true, false, true, false) => call4::<F, I, F, I, R>(f, b),
                (true, false, true, true) => call4::<F, I, F, F, R>(f, b),
                (true, true, false, false) => call4::<F, F, I, I, R>(f, b),
                (true, true, false, true) => call4::<F, F, I, F, R>(f, b),
                (true, true, true, false) => call4::<F, F, F, I, R>(f, b),
                (true, true, true, true) => call4::<F, F, F, F, R>(f, b),
            }
        })
    }
}

#[cfg(not(windows))]
mod abi {
    use super::Slot;
    use varn_types::NativeError;

    const MAX_INTS: usize = 6;
    const MAX_FLOATS: usize = 8;

    pub fn dispatch<R>(f: usize, slots: &[Slot]) -> Result<R, NativeError> {
        let mut ints = [0usize; MAX_INTS];
        let mut floats = [0f64; MAX_FLOATS];
        let (mut int_count, mut float_count) = (0, 0);
        for slot in slots {
            match slot {
                Slot::Int(bits) => {
                    *ints.get_mut(int_count).ok_or_else(|| {
                        NativeError::range_error(format!("ffi.call: at most {MAX_INTS} integer arguments are supported"))
                    })? = *bits as usize;
                    int_count += 1;
                }
                Slot::Float(value) => {
                    *floats.get_mut(float_count).ok_or_else(|| {
                        NativeError::range_error(format!("ffi.call: at most {MAX_FLOATS} float arguments are supported"))
                    })? = *value;
                    float_count += 1;
                }
            }
        }
        let f = unsafe {
            std::mem::transmute::<
                usize,
                extern "C" fn(
                    usize,
                    usize,
                    usize,
                    usize,
                    usize,
                    usize,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                    f64,
                ) -> R,
            >(f)
        };
        Ok(f(
            ints[0], ints[1], ints[2], ints[3], ints[4], ints[5], floats[0], floats[1], floats[2],
            floats[3], floats[4], floats[5], floats[6], floats[7],
        ))
    }
}

pub fn call(
    ctx: &mut dyn NativeCtx,
    fn_ptr: i64,
    ret_raw: i64,
    types: VnArray,
    args: VnArray,
) -> Result<VmValue, NativeError> {
    if fn_ptr == 0 {
        return Err(NativeError::range_error("ffi.call: null function pointer"));
    }
    let ret = Kind::from_raw(ret_raw).ok_or_else(|| NativeError::type_error("ffi.call: unknown return type"))?;
    let kinds = read_kinds(ctx, &types)?;
    let argc = args.len(ctx);
    if kinds.len() != argc {
        return Err(NativeError::type_error(format!(
            "ffi.call: {} parameter types declared but {argc} arguments given",
            kinds.len()
        )));
    }
    let values: Vec<VmValue> = (0..argc)
        .map(|i| args.get(ctx, i).unwrap_or_else(VmValue::null))
        .collect();
    let mut keep = Vec::new();
    let slots = marshal(ctx, &kinds, &values, &mut keep)?;
    let f = fn_ptr as usize;
    match ret {
        Kind::Void => {
            abi::dispatch::<()>(f, &slots)?;
            Ok(VmValue::null())
        }
        Kind::Float => Ok(VmValue::from_f64(abi::dispatch::<f64>(f, &slots)?)),
        Kind::Int | Kind::Ptr => Ok(ctx.int_val(abi::dispatch::<i64>(f, &slots)?)),
        Kind::Bool => {
            let raw = abi::dispatch::<u32>(f, &slots)?;
            Ok(ctx.bool_val(raw != 0))
        }
        Kind::Str => {
            let addr = abi::dispatch::<usize>(f, &slots)?;
            if addr == 0 {
                return Ok(VmValue::null());
            }
            let text = unsafe { CStr::from_ptr(addr as *const std::ffi::c_char) };
            Ok(ctx.alloc_str(&text.to_string_lossy()))
        }
    }
}
