//! `OpCode::Convert`: the one place an explicit numeric `as` changes
//! representation at run time. The rules live in `varn_core::numeric_conv`.

use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::NumConv;

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

fn heap_to_int(v: VmValue, heap: &Heap) -> VmResult<VmValue> {
    let obj = if v.is_heap() {
        heap.get(v.as_heap_idx())
    } else {
        None
    };
    match obj {
        Some(HeapObj::BigInt(b)) => i64::try_from(*b)
            .map(VmValue::from_int)
            .map_err(|_| out_of_int_range(b)),
        Some(HeapObj::Decimal(d)) => {
            use rust_decimal::prelude::ToPrimitive;
            d.trunc()
                .to_i64()
                .map(VmValue::from_int)
                .ok_or_else(|| out_of_int_range(d))
        }
        _ => Err(RuntimeError::new("convert: value is not numeric")),
    }
}

pub(crate) fn convert(conv: NumConv, v: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    match conv {
        NumConv::IntToFloat => Ok(VmValue::from_f64(heap.as_int(v) as f64)),
        NumConv::FloatToInt => float_to_int(v.as_f64()),
        NumConv::IntToBigInt => {
            let n = heap.as_int(v) as i128;
            Ok(VmValue::from_heap_idx(heap.alloc(HeapObj::BigInt(n))))
        }
        NumConv::IntToDecimal => {
            let n = heap.as_int(v);
            Ok(heap.alloc_decimal(n.into()))
        }
        NumConv::BigIntToInt | NumConv::DecimalToInt => heap_to_int(v, heap),
        NumConv::DynToInt if heap.is_int(v) => Ok(v),
        NumConv::DynToInt if v.is_f64() => float_to_int(v.as_f64()),
        NumConv::DynToInt => heap_to_int(v, heap),
        NumConv::DynToFloat if v.is_f64() => Ok(v),
        NumConv::DynToFloat if heap.is_int(v) => Ok(VmValue::from_f64(heap.as_int(v) as f64)),
        NumConv::DynToFloat => Ok(VmValue::from_f64(heap.to_f64_val(v))),
    }
}
