//! `OpCode::Convert`: the one place an explicit numeric `as` changes
//! representation at run time. The rules live in `varn_core::numeric_conv`.

use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use varn_core::NumConv;
use varn_types::Value;

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
        heap.get(v.as_heap_idx())
    } else {
        None
    };
    match obj {
        Some(HeapObj::BigInt(b)) => Ok(Numeric::Big((**b).clone())),
        Some(HeapObj::Decimal(d)) => Ok(Numeric::Dec((**d).clone())),
        _ => Err(RuntimeError::new("convert: value is not numeric")),
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
    Ok(heap.intern(Value::BigInt(Box::new(b))))
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

pub(crate) fn convert(conv: NumConv, v: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    let n = numeric_of(v, heap)?;
    match conv {
        NumConv::FloatToInt | NumConv::BigIntToInt | NumConv::DecimalToInt | NumConv::DynToInt => {
            to_int(n)
        }
        NumConv::IntToFloat
        | NumConv::DynToFloat
        | NumConv::BigIntToFloat
        | NumConv::DecimalToFloat => Ok(to_float(n)),
        NumConv::IntToBigInt | NumConv::FloatToBigInt | NumConv::DecimalToBigInt => {
            to_bigint(n, heap)
        }
        NumConv::IntToDecimal | NumConv::FloatToDecimal | NumConv::BigIntToDecimal => {
            to_decimal(n, heap)
        }
    }
}
