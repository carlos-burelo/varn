use super::fast::FastJsonParser;
use super::*;
use vn_core::type_desc::FieldDesc;
use vn_core::TypeDesc;

const MAX_TYPED_FIELDS: usize = 16;

impl ExecCtx {
    pub(crate) fn json_parse_typed(&mut self, text: &str, desc: &TypeDesc) -> Option<VmValue> {
        let mut parser = FastJsonParser::new(self, text);
        let value = parser.parse_typed(desc)?;
        parser.skip_whitespace();
        (parser.at_end()).then_some(value)
    }
}

impl FastJsonParser<'_, '_> {
    fn parse_typed(&mut self, desc: &TypeDesc) -> Option<VmValue> {
        match desc {
            TypeDesc::Any => self.parse_value(),
            TypeDesc::Int => self.parse_scalar_number().filter(|v| v.is_int()),
            TypeDesc::Float => self.parse_scalar_number().and_then(|v| {
                if v.is_f64() {
                    Some(v)
                } else if v.is_int() {
                    Some(VmValue::from_f64(v.as_int() as f64))
                } else {
                    None
                }
            }),
            TypeDesc::Bool => match self.peek()? {
                b't' => self.parse_true(),
                b'f' => self.parse_false(),
                _ => None,
            },
            TypeDesc::Str => match self.peek()? {
                b'"' => self.parse_string(),
                _ => None,
            },
            TypeDesc::Nullable(inner) => match self.peek()? {
                b'n' => self.parse_null(),
                _ => self.parse_typed(inner),
            },
            TypeDesc::Array(inner) => self.parse_typed_array(inner),
            TypeDesc::Object(fields) => self.parse_typed_object(desc, fields),
            TypeDesc::Literal(_)
            | TypeDesc::Char
            | TypeDesc::Bytes
            | TypeDesc::Decimal
            | TypeDesc::BigInt
            | TypeDesc::Callable
            | TypeDesc::Tuple(_)
            | TypeDesc::Map(..)
            | TypeDesc::Set(_)
            | TypeDesc::Class { .. }
            | TypeDesc::Enum { .. }
            | TypeDesc::Args { .. }
            | TypeDesc::Enclosing { .. } => None,
        }
    }

    fn parse_scalar_number(&mut self) -> Option<VmValue> {
        match self.peek()? {
            b'0'..=b'9' | b'-' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_typed_array(&mut self, inner: &TypeDesc) -> Option<VmValue> {
        if self.peek()? != b'[' {
            return None;
        }
        self.advance(1);
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek_raw()? == b']' {
                self.advance(1);
                return Some(self.ctx.alloc_array(items));
            }
            items.push(self.parse_typed(inner)?);
            self.skip_whitespace();
            match self.peek_raw()? {
                b',' => self.advance(1),
                b']' => {
                    self.advance(1);
                    return Some(self.ctx.alloc_array(items));
                }
                _ => return None,
            }
        }
    }

    fn parse_typed_object(&mut self, desc: &TypeDesc, fields: &[FieldDesc]) -> Option<VmValue> {
        if self.peek()? != b'{' || fields.len() > MAX_TYPED_FIELDS {
            return None;
        }
        self.advance(1);
        let mut slots = [VmValue::null(); MAX_TYPED_FIELDS];
        let mut present: u64 = 0;
        let mut expected = 0usize;
        self.skip_whitespace();
        if self.peek_raw()? == b'}' {
            self.advance(1);
        } else {
            loop {
                self.skip_whitespace();
                if self.peek_raw()? != b'"' {
                    return None;
                }
                let key = self.read_string_text()?;
                let at = match fields.get(expected) {
                    Some(field) if field.name == *key => expected,
                    Some(_) | None => fields.iter().position(|field| field.name == *key)?,
                };
                if present & (1 << at) != 0 {
                    return None;
                }
                self.skip_whitespace();
                if self.peek_raw()? != b':' {
                    return None;
                }
                self.advance(1);
                slots[at] = self.parse_typed(&fields[at].ty)?;
                present |= 1 << at;
                expected = at + 1;
                self.skip_whitespace();
                match self.peek_raw()? {
                    b',' => self.advance(1),
                    b'}' => {
                        self.advance(1);
                        break;
                    }
                    _ => return None,
                }
            }
        }
        for (index, field) in fields.iter().enumerate() {
            let absent_allowed =
                field.optional || matches!(field.ty, TypeDesc::Nullable(_) | TypeDesc::Any);
            if present & (1 << index) == 0 && !absent_allowed {
                return None;
            }
        }
        if present == 0 {
            return Some(self.ctx.alloc_object());
        }
        let mut ordered = [VmValue::null(); MAX_TYPED_FIELDS];
        let mut count = 0;
        for (index, slot) in slots.iter().enumerate().take(fields.len()) {
            if present & (1 << index) != 0 {
                ordered[count] = *slot;
                count += 1;
            }
        }
        let shape = self.typed_shape(desc, fields, present)?;
        Some(
            self.ctx
                .heap
                .alloc_object_with_shape_slice(&shape, &ordered[..count]),
        )
    }

    fn typed_shape(
        &mut self,
        desc: &TypeDesc,
        fields: &[FieldDesc],
        present: u64,
    ) -> Option<std::rc::Rc<vn_types::Shape>> {
        let key = (desc as *const TypeDesc as usize, present);
        if let Some(shape) = self.ctx.typed_shapes.get(&key) {
            return Some(std::rc::Rc::clone(shape));
        }
        let pairs: Vec<(std::sync::Arc<str>, VmValue)> = fields
            .iter()
            .enumerate()
            .filter(|(index, _)| present & (1 << index) != 0)
            .map(|(_, field)| (std::sync::Arc::from(field.name.as_str()), VmValue::null()))
            .collect();
        let probe = self.ctx.heap.alloc_object_pairs(pairs);
        let shape = self.ctx.get_object_shape(probe)?;
        if shape.class.is_some() || shape.ordered_names().len() != present.count_ones() as usize {
            return None;
        }
        self.ctx
            .typed_shapes
            .insert(key, std::rc::Rc::clone(&shape));
        Some(shape)
    }
}
