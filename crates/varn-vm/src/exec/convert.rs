use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use varn_core::ValueConv;

#[cold]
#[inline(never)]
fn out_of_int_range(what: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::integer_overflow(format!("integer overflow: {what} does not fit int"))
}

fn float_to_int(f: f64) -> VmResult<VmValue> {
    varn_core::float_to_int(f)
        .map(VmValue::from_int)
        .ok_or_else(|| out_of_int_range(f))
}

enum Numeric {
    Int(i64),
    Float(f64),
    Big(BigInt),
    Dec(BigDecimal),
}

fn numeric_of(v: VmValue, heap: &Heap) -> VmResult<Numeric> {
    if heap.is_int(v) {
        return Ok(Numeric::Int(heap.as_int(v)));
    }
    if v.is_f64() {
        return Ok(Numeric::Float(v.as_f64()));
    }
    let obj = if v.is_heap() {
        heap.get(v.as_heap())
    } else {
        None
    };
    match obj {
        Some(HeapObj::BigInt(b)) => Ok(Numeric::Big((**b).clone())),
        Some(HeapObj::Decimal(d)) => Ok(Numeric::Dec((**d).clone())),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => Err(RuntimeError::new("convert: value is not numeric")),
    }
}

fn to_int(n: Numeric) -> VmResult<VmValue> {
    match n {
        Numeric::Int(i) => Ok(VmValue::from_int(i)),
        Numeric::Float(f) => float_to_int(f),
        Numeric::Big(b) => b
            .to_i64()
            .map(VmValue::from_int)
            .ok_or_else(|| out_of_int_range(b)),
        Numeric::Dec(d) => d
            .with_scale_round(0, RoundingMode::Down)
            .to_i64()
            .map(VmValue::from_int)
            .ok_or_else(|| out_of_int_range(d)),
    }
}

fn to_float(n: Numeric) -> VmValue {
    let f = match n {
        Numeric::Int(i) => i as f64,
        Numeric::Float(f) => f,
        Numeric::Big(b) => b.to_f64().unwrap_or(f64::NAN),
        Numeric::Dec(d) => d.to_f64().unwrap_or(f64::NAN),
    };
    VmValue::from_f64(f)
}

fn to_bigint(n: Numeric, heap: &mut Heap) -> VmResult<VmValue> {
    let b = match n {
        Numeric::Int(i) => BigInt::from(i),
        Numeric::Big(b) => b,
        Numeric::Float(f) => BigInt::from_f64(f.trunc()).ok_or_else(|| {
            RuntimeError::integer_overflow(format!("integer overflow: {f} has no bigint value"))
        })?,
        Numeric::Dec(d) => {
            d.with_scale_round(0, RoundingMode::Down)
                .as_bigint_and_exponent()
                .0
        }
    };
    Ok(heap.alloc_bigint(b))
}

fn to_decimal(n: Numeric, heap: &mut Heap) -> VmResult<VmValue> {
    let d = match n {
        Numeric::Int(i) => BigDecimal::from(i),
        Numeric::Big(b) => BigDecimal::from(b),
        Numeric::Dec(d) => d,
        Numeric::Float(f) => BigDecimal::from_f64(f)
            .ok_or_else(|| RuntimeError::new(format!("cannot convert {f} to decimal")))?,
    };
    Ok(heap.alloc_decimal(d))
}

#[cold]
#[inline(never)]
fn not_a(expected: &str, v: VmValue, heap: &Heap) -> RuntimeError {
    RuntimeError::type_error(format!(
        "type mismatch: expected '{expected}', got '{}'",
        crate::exec::advanced::typeof_val(v, heap)
    ))
}

pub(crate) fn convert(conv: ValueConv, v: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    match conv {
        ValueConv::DynToStr | ValueConv::DynToStrOrNull => {
            let nullable = conv == ValueConv::DynToStrOrNull;
            return if heap.is_string(v) || (nullable && v.is_null()) {
                Ok(v)
            } else {
                Err(not_a(if nullable { "str?" } else { "str" }, v, heap))
            };
        }
        ValueConv::DynToBool | ValueConv::DynToBoolOrNull => {
            let nullable = conv == ValueConv::DynToBoolOrNull;
            return if v.is_bool() || (nullable && v.is_null()) {
                Ok(v)
            } else {
                Err(not_a(if nullable { "bool?" } else { "bool" }, v, heap))
            };
        }
        ValueConv::IntToFloat
        | ValueConv::FloatToInt
        | ValueConv::IntToBigInt
        | ValueConv::BigIntToInt
        | ValueConv::IntToDecimal
        | ValueConv::DecimalToInt
        | ValueConv::DynToInt
        | ValueConv::DynToFloat
        | ValueConv::BigIntToFloat
        | ValueConv::FloatToBigInt
        | ValueConv::DecimalToFloat
        | ValueConv::FloatToDecimal
        | ValueConv::BigIntToDecimal
        | ValueConv::DecimalToBigInt => {}
    }
    let n = numeric_of(v, heap)?;
    match conv {
        ValueConv::DynToStr
        | ValueConv::DynToBool
        | ValueConv::DynToStrOrNull
        | ValueConv::DynToBoolOrNull => Ok(v),
        ValueConv::FloatToInt | ValueConv::BigIntToInt | ValueConv::DecimalToInt | ValueConv::DynToInt => {
            to_int(n)
        }
        ValueConv::IntToFloat
        | ValueConv::DynToFloat
        | ValueConv::BigIntToFloat
        | ValueConv::DecimalToFloat => Ok(to_float(n)),
        ValueConv::IntToBigInt | ValueConv::FloatToBigInt | ValueConv::DecimalToBigInt => {
            to_bigint(n, heap)
        }
        ValueConv::IntToDecimal | ValueConv::FloatToDecimal | ValueConv::BigIntToDecimal => {
            to_decimal(n, heap)
        }
    }
}
