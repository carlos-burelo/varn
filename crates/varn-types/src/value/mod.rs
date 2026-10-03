mod alloc;
mod buffer;
mod class;
mod instance;
mod map;
mod module;
mod object;
mod sendable;
mod shape;
pub use crate::native::NativeFn;
pub use alloc::{MapKey, MapRef, ObjRef, RuntimeString, SetRef, ValueMap, ValueSet};
pub use buffer::VmBuffer;
pub use class::{find_method_with_owner, ClassObj};
pub use instance::{InstanceData, InstanceRef, INST_CLASS_ID_OFF, INST_PAYLOAD_OFF};
pub use module::{FrozenExport, FrozenModuleObj, ModuleObj};
pub use object::{ObjData, OBJ_INLINE_LEN_OFF, OBJ_SHAPE_OFF, OBJ_VALUES_OFF};
pub use sendable::{SendEnumVariant, SendValue};
pub use shape::{root_shape, Shape, SHAPE_ID_OFF};
use std::rc::Rc;
use std::sync::Arc;
pub use varn_core::RuntimeKind;

/// The element domain of a `Range<T>`: bounds are stored as `i64` either
/// way (a `char` as its code point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RangeElem {
    Int,
    Char,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RangeData {
    pub start: i64,
    pub end: i64,
    pub inclusive: bool,
    pub step: i64,
    pub elem: RangeElem,
}

impl RangeData {
    pub fn int(start: i64, end: i64, inclusive: bool) -> Self {
        Self {
            start,
            end,
            inclusive,
            step: 1,
            elem: RangeElem::Int,
        }
    }

    pub fn end_exclusive(&self) -> i64 {
        if self.inclusive {
            self.end.saturating_add(1)
        } else {
            self.end
        }
    }

    /// Number of elements, `step` included.
    pub fn len(&self) -> i64 {
        let span = self.end_exclusive() - self.start;
        if span <= 0 {
            0
        } else {
            (span + self.step - 1) / self.step
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The raw bound of element `i`, or `None` past the end.
    pub fn nth(&self, i: i64) -> Option<i64> {
        (0..self.len())
            .contains(&i)
            .then(|| self.start + i * self.step)
    }

    pub fn contains(&self, raw: i64) -> bool {
        raw >= self.start && raw < self.end_exclusive() && (raw - self.start) % self.step == 0
    }

    /// The character a raw bound of a `char` range stands for.
    pub fn char_of(raw: i64) -> char {
        u32::try_from(raw)
            .ok()
            .and_then(char::from_u32)
            .unwrap_or('\0')
    }

    pub fn with_step(&self, step: i64) -> Self {
        Self {
            step,
            ..self.clone()
        }
    }
}

impl std::fmt::Display for RangeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dots = if self.inclusive { "..=" } else { ".." };
        match self.elem {
            RangeElem::Int => write!(f, "{}{dots}{}", self.start, self.end)?,
            RangeElem::Char => write!(
                f,
                "{}{dots}{}",
                Self::char_of(self.start),
                Self::char_of(self.end)
            )?,
        }
        if self.step != 1 {
            write!(f, " step {}", self.step)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RuntimeSymbol {
    Iterator,
    AsyncIterator,
}

impl RuntimeSymbol {
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeSymbol::Iterator => "Symbol.iterator",
            RuntimeSymbol::AsyncIterator => "Symbol.asyncIterator",
        }
    }
}

impl std::fmt::Display for RuntimeSymbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone)]
pub struct EnumVariantData {
    /// The enum's class (`ClassObj::id`), set when the variant is attached to
    /// it; methods and identity come from here, never from `enum_name`. `None`
    /// only for a value rebuilt on the far side of an isolate channel.
    pub enum_class_id: Option<u32>,
    pub enum_name: Arc<str>,
    pub variant_name: Arc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Arc<str>>,
    pub payload: crate::vm_value::VmValue,
}

#[derive(Clone, Debug)]
pub enum BoundMethodTarget {
    Native {
        func: NativeFn,
        name: &'static str,
    },
    Vm {
        closure: crate::vm_value::VmValue,
        owner_class: Option<Rc<ClassObj>>,
    },
}

#[derive(Clone, Debug)]
pub struct BoundMethod {
    pub receiver: crate::vm_value::VmValue,
    pub target: BoundMethodTarget,
}
