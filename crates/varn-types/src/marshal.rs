//! Typed marshalling layer between Rust and `VmValue`.
//!
//! This is the foundation of the contract-driven native bindings (the
//! stdlib/builtins redesign). The contract `.vn` is the single source of
//! truth; a generator turns each declared member into a Rust trait method
//! whose argument and return types are real Rust types. The generated
//! dispatch wrappers use [`FromVm`]/[`IntoVm`] to bridge those typed
//! signatures to the raw `&[VmValue]` calling convention the VM speaks.
//!
//! Nothing here is wired into the dispatcher yet — Phase 0 only lays the
//! typed bridge so later phases can generate against it.

use crate::native_ctx::NativeCtx;
use crate::vm_value::VmValue;
use crate::Value;

/// Decode a `VmValue` argument into a typed Rust value.
///
/// Returns `Err` with a human-readable message when the runtime value does
/// not match the type the contract declared (wrong tag, missing field, etc.).
/// The generated wrapper turns that `Err` into the dispatcher's error string.
pub trait FromVm: Sized {
    fn from_vm(ctx: &dyn NativeCtx, v: VmValue) -> Result<Self, String>;
}

/// Encode a typed Rust value back into a `VmValue`, allocating on the heap
/// through `ctx` when the value is not representable inline (strings, arrays).
pub trait IntoVm {
    fn into_vm(self, ctx: &mut dyn NativeCtx) -> VmValue;
}

// ---------------------------------------------------------------------------
// FromVm — argument decoding (strict: the tag must match the contract type)
// ---------------------------------------------------------------------------

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
        // `int` is assignable to `float` in the checker, so accept both.
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
        // `str_owned` handles both SSO-inlined and heap strings.
        ctx.str_owned(v)
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

/// An optional argument or a nullable contract type (`T?`). Both a missing
/// trailing argument (the wrapper passes `null`) and an explicit `null`
/// decode to `None`.
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

// ---------------------------------------------------------------------------
// IntoVm — return encoding
// ---------------------------------------------------------------------------

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

/// Unit return maps to `null` (contract `void`).
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

// ---------------------------------------------------------------------------
// VnArray — a typed handle over a heap array (`T[]` in a contract)
// ---------------------------------------------------------------------------

/// Type-erased array handle. The contract keeps the element type `T` for the
/// checker; at runtime elements are raw `VmValue` reached through `ctx`.
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

    /// Collect every element into an owned `Vec` (allocates).
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
