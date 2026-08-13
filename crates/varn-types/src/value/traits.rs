use super::{BoundMethod, BoundMethodTarget, ObjRef, Value, VmValuePayload};
use crate::native::NativeFn;
use std::fmt;
use std::hash::Hasher;
use std::rc::Rc;
use varn_core::TypeTag;

impl Value {
    #[inline(always)]
    pub fn native(func: NativeFn, name: &'static str) -> Self {
        Value::NativeFn(Box::new((func, name)))
    }

    #[inline(always)]
    pub fn native_bound(receiver: Value, func: NativeFn, name: &'static str) -> Self {
        Value::BoundMethod(Box::new(BoundMethod {
            receiver,
            target: BoundMethodTarget::Native { func, name },
        }))
    }

    #[inline(always)]
    pub fn vm_bound(
        receiver: Value,
        closure: Box<dyn VmValuePayload>,
        owner_class: Option<Rc<super::ClassObj>>,
    ) -> Self {
        Value::BoundMethod(Box::new(BoundMethod {
            receiver,
            target: BoundMethodTarget::Vm {
                closure,
                owner_class,
            },
        }))
    }

    pub fn instance(class: Rc<super::ClassObj>) -> Self {
        let obj_ref = ObjRef::instance(class);
        Value::Object(obj_ref)
    }

    pub fn is_truthy(&self) -> Result<bool, String> {
        match self {
            Value::Null => Ok(false),
            Value::Bool(b) => Ok(*b),
            Value::Int(n) => Ok(*n != 0),
            Value::Float(d) => Ok(*d != 0.0 && !d.is_nan()),
            Value::Str(s) => Ok(!s.is_empty()),
            Value::Array(a) => Ok(!a.read().is_empty()),
            _ => Ok(true),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// The single canonical type name of this value. All flavours collapse to
    /// [`TypeTag::name`]: callables (closure/native/bound) report `"function"`,
    /// a module reports as an `"object"`, and a spread forwards to its inner
    /// value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => TypeTag::Null,
            Value::Bool(_) => TypeTag::Bool,
            Value::Int(_) => TypeTag::Int,
            Value::Float(_) => TypeTag::Float,
            Value::Str(_) => TypeTag::Str,
            Value::BigInt(_) => TypeTag::BigInt,
            Value::Decimal(_) => TypeTag::Decimal,
            Value::Array(_) => TypeTag::Array,
            Value::Object(_) | Value::Module(_) => TypeTag::Object,
            Value::Class(_) => TypeTag::Class,
            Value::NativeFn(_) | Value::BoundMethod(_) => TypeTag::Function,
            Value::Spread(v) => return v.type_name(),
            Value::TaskHandle(_) | Value::Task(_) => TypeTag::TaskHandle,
            Value::Range(_) => TypeTag::Range,
            Value::Map(_) => TypeTag::Map,
            Value::Set(_) => TypeTag::Set,
            Value::Symbol(_) => TypeTag::Symbol,
            Value::Generator(_) => TypeTag::Generator,
            Value::AsyncQueue(_) => TypeTag::AsyncQueue,
            Value::Char(_) => TypeTag::Char,
            Value::EnumVariant(_) => TypeTag::Enum,
            Value::VmValue(_payload) => TypeTag::VmRef,
        }
        .name()
    }

    pub fn equals(&self, other: &Value) -> bool {
        self == other
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<Rc<str>> {
        match self {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl std::hash::Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Null => {}
            Value::Bool(b) => b.hash(state),
            Value::Int(n) => n.hash(state),
            Value::Float(f) => f.to_bits().hash(state),
            Value::Str(s) => s.hash(state),
            Value::BigInt(n) => n.hash(state),
            Value::Decimal(d) => d.hash(state),
            Value::Array(a) => Rc::as_ptr(&a.0).hash(state),
            Value::Object(o) => Rc::as_ptr(&o.0).hash(state),
            Value::Class(c) => Rc::as_ptr(c).hash(state),
            Value::NativeFn(b) => (b.0 as usize).hash(state),
            Value::BoundMethod(b) => {
                b.receiver.hash(state);
                match &b.target {
                    BoundMethodTarget::Native { func, .. } => (*func as usize).hash(state),
                    BoundMethodTarget::Vm { .. } => {
                        0.hash(state);
                    }
                }
            }
            Value::Spread(v) => v.hash(state),
            Value::Task(task) => Rc::as_ptr(task).hash(state),
            Value::TaskHandle(f) => f.hash(state),
            Value::Range(r) => r.hash(state),
            Value::Map(m) => Rc::as_ptr(&m.0).hash(state),
            Value::Set(s) => Rc::as_ptr(&s.0).hash(state),
            Value::Symbol(s) => s.hash(state),
            Value::Generator(g) => g.hash(state),
            Value::AsyncQueue(q) => Rc::as_ptr(&q.0).hash(state),
            Value::Char(c) => c.hash(state),
            Value::EnumVariant(data) => {
                data.variant_name.hash(state);
                data.variant_tag.hash(state);
                data.payload.hash(state);
            }
            Value::VmValue(_) => {}
            Value::Module(m) => Rc::as_ptr(m).hash(state),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::Decimal(a), Value::Decimal(b)) => a == b,
            (Value::Range(a), Value::Range(b)) => a == b,
            (Value::Task(a), Value::Task(b)) => Rc::ptr_eq(a, b),
            (Value::TaskHandle(a), Value::TaskHandle(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(&a.0, &b.0),
            (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(&a.0, &b.0),
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(&a.0, &b.0),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(&a.0, &b.0),
            (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::Generator(a), Value::Generator(b)) => a == b,
            (Value::AsyncQueue(a), Value::AsyncQueue(b)) => Rc::ptr_eq(&a.0, &b.0),
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::EnumVariant(a), Value::EnumVariant(b)) => {
                a.variant_name == b.variant_name
                    && a.variant_tag == b.variant_tag
                    && a.payload == b.payload
            }
            (Value::VmValue(a), Value::VmValue(b)) => std::ptr::eq(a.as_any(), b.as_any()),
            _ => false,
        }
    }
}
impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Str(a), Value::Str(b)) => a.partial_cmp(b),
            (Value::Char(a), Value::Char(b)) => a.partial_cmp(b),
            (Value::BigInt(a), Value::BigInt(b)) => a.partial_cmp(b),
            (Value::Decimal(a), Value::Decimal(b)) => a.partial_cmp(b),
            _ => {
                if self == other {
                    Some(std::cmp::Ordering::Equal)
                } else {
                    None
                }
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(d) => {
                if d.fract() == 0.0 && d.abs() < 9_007_199_254_740_992.0 {
                    write!(f, "{}", *d as i64)
                } else {
                    write!(f, "{d}")
                }
            }
            Value::Str(s) => write!(f, "{s}"),
            Value::BigInt(n) => write!(f, "{n}n"),
            Value::Decimal(d) => write!(f, "{d}"),
            Value::Array(arr) => {
                let v = arr.read();
                write!(f, "[")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{val}")?;
                }
                write!(f, "]")
            }
            Value::Object(obj_ref) => {
                let obj = obj_ref.read();
                if let Some(class) = obj.class() {
                    write!(f, "[object {}]", class.name)
                } else {
                    write!(f, "{{ ")?;
                    for (i, (k, v)) in obj.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{k}: {v}")?;
                    }
                    write!(f, " }}")
                }
            }
            Value::Class(c) => write!(f, "[class {}]", c.name),
            Value::NativeFn(b) => write!(f, "[NativeFn: {}]", b.1),
            Value::BoundMethod(b) => match &b.target {
                BoundMethodTarget::Native { name, .. } => write!(f, "[Function: {}]", name),
                BoundMethodTarget::Vm { .. } => write!(f, "[BoundMethod]"),
            },
            Value::Spread(v) => write!(f, "{v}"),
            Value::Task(_) => write!(f, "Task(<lazy>)"),
            Value::TaskHandle(fut) => match fut.peek_state() {
                crate::task::TaskState::Pending => write!(f, "Task(<pending>)"),
                crate::task::TaskState::Resolved(v) => write!(f, "Task({v})"),
                crate::task::TaskState::Rejected(v) => write!(f, "Task(<rejected:{v}>)"),
            },
            Value::Range(r) => write!(f, "{}..{}", r.start, r.end),
            Value::Map(map_ref) => {
                let m = map_ref.read();
                write!(f, "Map({})", m.len())
            }
            Value::Set(set_ref) => {
                let s = set_ref.read();
                write!(f, "Set({})", s.len())
            }
            Value::Symbol(s) => write!(f, "{s}"),
            Value::Generator(_) => write!(f, "[Generator]"),
            Value::AsyncQueue(_) => write!(f, "[AsyncQueue]"),
            Value::Char(c) => write!(f, "'{c}'"),
            Value::EnumVariant(data) => {
                write!(f, "{}({})", data.variant_name, data.payload)
            }
            Value::VmValue(payload) => write!(f, "{:?}", payload),
            Value::Module(m) => write!(f, "[module {}]", m.id.as_str()),
        }
    }
}
