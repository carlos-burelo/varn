use super::Value;

const INLINE: usize = 3;

#[derive(Debug, Clone)]
enum Repr {
    Inline(u8, [Value; INLINE]),
    Heap(Vec<Value>),
}

#[derive(Debug, Clone)]
pub struct TaskArgs(Repr);

impl TaskArgs {
    pub fn collect(mut values: impl ExactSizeIterator<Item = Value>) -> Self {
        let len = values.len();
        if len > INLINE {
            return Self(Repr::Heap(values.collect()));
        }
        let mut inline = [Value::Null, Value::Null, Value::Null];
        for slot in inline.iter_mut().take(len) {
            if let Some(v) = values.next() {
                *slot = v;
            }
        }
        Self(Repr::Inline(len as u8, inline))
    }

    pub fn as_slice(&self) -> &[Value] {
        match &self.0 {
            Repr::Inline(len, inline) => &inline[..*len as usize],
            Repr::Heap(values) => values,
        }
    }
}
