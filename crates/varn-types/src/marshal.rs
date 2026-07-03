use crate::native_ctx::NativeCtx;
use crate::vm_value::VmValue;
use crate::Value;

pub trait FromVm: Sized {
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String>;
}

pub trait IntoVm {
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue;
}

impl FromVm for VmValue {
    #[inline]
    fn from_vm(_ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        Ok(v)
    }
}

impl FromVm for i64 {
    #[inline]
    fn from_vm(_ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if v.is_int() {
            Ok(v.as_int())
        } else {
            Err(format!("expected int, got {v:?}"))
        }
    }
}

impl FromVm for f64 {
    #[inline]
    fn from_vm(_ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if v.is_f64() || v.is_int() {
            Ok(v.to_f64())
        } else {
            Err(format!("expected float, got {v:?}"))
        }
    }
}

impl FromVm for bool {
    #[inline]
    fn from_vm(_ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if v.is_bool() {
            Ok(v.as_bool())
        } else {
            Err(format!("expected bool, got {v:?}"))
        }
    }
}

impl FromVm for String {
    #[inline]
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        ctx.str_owned(v)
            .ok_or_else(|| format!("expected str, got {v:?}"))
    }
}

/// Borrowed-string marshaling for native params: SSO payloads decode into an
/// inline buffer and heap strings clone their `Rc<str>` — no allocation, no
/// byte copy. The `Rc` keeps the string alive independently of the heap
/// slot, so callee-side heap mutation (allocation, GC) cannot invalidate it.
pub enum VnStr {
    Sso { buf: [u8; 5], len: u8 },
    Shared(std::rc::Rc<str>),
}

impl VnStr {
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            // Valid UTF-8 by construction: the buffer was produced by
            // `sso_as_str` at marshal time.
            VnStr::Sso { buf, len } => unsafe {
                std::str::from_utf8_unchecked(&buf[..*len as usize])
            },
            VnStr::Shared(s) => s,
        }
    }
}

impl FromVm for VnStr {
    #[inline]
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if v.is_sso() {
            let mut buf = [0u8; 5];
            let len = v.sso_as_str(&mut buf).len() as u8;
            return Ok(VnStr::Sso { buf, len });
        }
        ctx.str_shared(v)
            .map(VnStr::Shared)
            .ok_or_else(|| format!("expected str, got {v:?}"))
    }
}

impl FromVm for char {
    #[inline]
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        match ctx.extract(v) {
            Value::Char(c) => Ok(c),
            other => Err(format!("expected char, got {other:?}")),
        }
    }
}

impl<T: FromVm> FromVm for Option<T> {
    #[inline]
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if v.is_null() {
            Ok(None)
        } else {
            T::from_vm(ctx, v).map(Some)
        }
    }
}

impl IntoVm for VmValue {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        self
    }
}

impl IntoVm for i64 {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::from_int(self)
    }
}

impl IntoVm for i32 {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::from_int(self as i64)
    }
}

impl IntoVm for usize {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::from_int(self as i64)
    }
}

impl IntoVm for f64 {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::from_f64(self)
    }
}

impl IntoVm for bool {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::from_bool(self)
    }
}

impl IntoVm for String {
    #[inline]
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue {
        ctx.alloc_str_owned(self)
    }
}

impl IntoVm for &str {
    #[inline]
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue {
        ctx.alloc_str(self)
    }
}

impl IntoVm for char {
    #[inline]
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue {
        ctx.intern(Value::Char(self))
    }
}

impl IntoVm for () {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        VmValue::null()
    }
}

impl<T: IntoVm> IntoVm for Option<T> {
    #[inline]
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue {
        match self {
            Some(t) => t.into_vm(ctx),
            None => VmValue::null(),
        }
    }
}

impl IntoVm for Vec<VmValue> {
    #[inline]
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue {
        ctx.alloc_array(self)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct VnArray(pub VmValue);

impl VnArray {
    #[inline]
    pub fn raw(self) -> VmValue {
        self.0
    }

    #[inline]
    pub fn len(self, ctx: &dyn NativeCtx) -> usize {
        ctx.array_len(self.0)
    }

    #[inline]
    pub fn is_empty(self, ctx: &dyn NativeCtx) -> bool {
        self.len(ctx) == 0
    }

    #[inline]
    pub fn get(self, ctx: &dyn NativeCtx, idx: usize) -> Option<VmValue> {
        ctx.array_get(self.0, idx)
    }

    #[inline]
    pub fn set(self, ctx: &mut dyn NativeCtx, idx: usize, val: VmValue) {
        ctx.array_set(self.0, idx, val);
    }

    #[inline]
    pub fn push(self, ctx: &mut dyn NativeCtx, val: VmValue) {
        ctx.array_push(self.0, val);
    }

    #[inline]
    pub fn pop(self, ctx: &mut dyn NativeCtx) -> Option<VmValue> {
        ctx.array_pop(self.0)
    }

    pub fn to_vec(self, ctx: &dyn NativeCtx) -> Vec<VmValue> {
        let len = self.len(ctx);
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.get(ctx, i).unwrap_or_else(VmValue::null));
        }
        out
    }
}

impl FromVm for VnArray {
    #[inline]
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String> {
        if ctx.is_array(v) {
            Ok(VnArray(v))
        } else {
            Err(format!("expected array, got {v:?}"))
        }
    }
}

impl IntoVm for VnArray {
    #[inline]
    fn into_vm(self, _ctx: &mut dyn NativeCtx) -> VmValue {
        self.0
    }
}
