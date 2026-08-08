use crate::error::{RuntimeError, VmResult};
use crate::heap::Heap;
use crate::value::VmValue;
use varn_core::intrinsic_ops::int::IntOp;

/// Format an i64 into a stack buffer; avoids the `String` allocation that
/// `i64::to_string` pays on every call. Results of 5 or fewer bytes become
/// SSO values in `alloc_str_dynamic`, skipping the heap entirely.
fn format_i64(mut n: i64, buf: &mut [u8; 20]) -> &str {
    let mut i = buf.len();
    let neg = n < 0;
    loop {
        // Negative modulo keeps i64::MIN in range (its magnitude overflows abs()).
        let digit = (n % 10).unsigned_abs() as u8;
        i -= 1;
        buf[i] = b'0' + digit;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    // Safety: buffer holds only ASCII digits and '-'.
    unsafe { std::str::from_utf8_unchecked(&buf[i..]) }
}

pub(crate) fn dispatch(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let recv = args
        .first()
        .copied()
        .ok_or_else(|| RuntimeError::new("int intrinsic: missing receiver"))?;
    match op {
        o if o == IntOp::ToString as u8 => {
            if recv.is_int() {
                let mut buf = [0u8; 20];
                let s = format_i64(recv.as_int(), &mut buf);
                Ok(heap.alloc_str_dynamic(s))
            } else {
                // Statically `int` but boxed differently (e.g. via dynamic
                // widening) — fall back to the generic representation.
                let s = heap.str_repr(recv);
                Ok(heap.alloc_str_dynamic(s))
            }
        }
        _ => Err(RuntimeError::new(format!("int intrinsic: unknown op {op}"))),
    }
}
