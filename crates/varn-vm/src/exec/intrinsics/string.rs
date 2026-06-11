use crate::error::VmResult;
use crate::heap::Heap;
use crate::value::VmValue;
use varn_core::intrinsic_ops::string::StringOp;

pub fn dispatch(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let Some(arg0) = args.first() else {
        return Ok(VmValue::null());
    };
    let s = heap.str_repr(*arg0);
    let result = match op {
        v if v == StringOp::Len as u8 => return Ok(VmValue::from_int(s.len() as i64)),
        v if v == StringOp::Contains as u8 => {
            let pat = args.get(1).map(|a| heap.str_repr(*a)).unwrap_or_default();
            return Ok(VmValue::from_bool(s.contains(pat.as_str())));
        }
        v if v == StringOp::StartsWith as u8 => {
            let pat = args.get(1).map(|a| heap.str_repr(*a)).unwrap_or_default();
            return Ok(VmValue::from_bool(s.starts_with(pat.as_str())));
        }
        v if v == StringOp::EndsWith as u8 => {
            let pat = args.get(1).map(|a| heap.str_repr(*a)).unwrap_or_default();
            return Ok(VmValue::from_bool(s.ends_with(pat.as_str())));
        }
        v if v == StringOp::ToUpperCase as u8 => s.to_uppercase(),
        v if v == StringOp::ToLowerCase as u8 => s.to_lowercase(),
        v if v == StringOp::Trim as u8 => s.trim().to_string(),
        _ => return Ok(VmValue::null()),
    };
    Ok(heap.alloc_str(result))
}
