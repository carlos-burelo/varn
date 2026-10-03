use varn_core::OpCode;

use super::definition::{FunctionProto, TrivialInitPlan};

impl FunctionProto {
    /// Whether the body contains a back edge (`OpCode::Loop`).
    ///
    /// Tiering counts FRAME ENTRIES, which says nothing about a function that
    /// is entered once and then spins a million iterations: it never reaches
    /// any threshold, and without on-stack replacement there is no second
    /// chance to compile it. So a looping function is compiled on its first
    /// entry and only straight-line code is made to prove itself by being
    /// called again — for that shape the entry count is exactly the right
    /// evidence. Measured: a flat threshold of 8 is 5.6x on the test suite and
    /// 2.4x WORSE on `bench_matrix`; splitting the two recovers both.
    ///
    /// Walked through the shared decoder so operand words can never be
    /// mistaken for an opcode, and memoised — the answer is a property of the
    /// bytecode, which never changes.
    pub fn has_backedge(&self) -> bool {
        match self.backedge_memo.get() {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let code = &self.chunk.code;
        let mut ip = 0;
        let mut found = false;
        while ip < code.len() {
            let Some(info) = crate::bytecode::decode(code, ip, &self.chunk.constants) else {
                // Undecodable: assume the worst and compile eagerly, which is
                // the pre-existing behaviour.
                found = true;
                break;
            };
            if OpCode::from_u16(code[ip]) == Some(OpCode::Loop) {
                found = true;
                break;
            }
            ip += info.len.max(1);
        }
        self.backedge_memo.set(if found { 1 } else { 2 });
        found
    }

    pub fn has_try(&self) -> bool {
        let code = &self.chunk.code;
        let mut ip = 0;
        while ip < code.len() {
            let Some(info) = crate::bytecode::decode(code, ip, &self.chunk.constants) else {
                return true;
            };
            if OpCode::from_u16(code[ip]) == Some(OpCode::Try) {
                return true;
            }
            ip += info.len.max(1);
        }
        false
    }

    /// Checks if this constructor proto is a trivial field-initializer:
    /// it consists purely of straight-line `SetFixedField this, param_reg, slot`
    /// instructions ending in Return.
    pub fn trivial_field_init_plan(&self) -> Option<TrivialInitPlan> {
        if let Some(ref cached) = *self.trivial_init_memo.borrow() {
            return cached.clone();
        }
        let plan = self.compute_trivial_field_init_plan().map(|p| p.into());
        *self.trivial_init_memo.borrow_mut() = Some(plan.clone());
        plan
    }

    fn compute_trivial_field_init_plan(
        &self,
    ) -> Option<Vec<(usize, u32, Option<varn_core::RuntimeKind>)>> {
        if self.is_async || self.is_generator || self.has_rest || self.upvalue_count > 0 {
            return None;
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum RegSource {
            This,
            Param(usize), // 0-based parameter index
        }

        let total_regs = (self.register_count as usize).max(self.arity + 1);
        let mut sources = vec![None; total_regs];
        sources[0] = Some(RegSource::This);
        for i in 1..=self.arity {
            if i < sources.len() {
                sources[i] = Some(RegSource::Param(i - 1));
            }
        }

        let code = &self.chunk.code;
        let mut ip = 0;
        let mut plan = Vec::new();

        while ip < code.len() {
            let op = OpCode::from_u16(code[ip])?;
            match op {
                OpCode::Move => {
                    let dst = (code[ip] >> 8) as usize;
                    let src = (code[ip + 1] >> 8) as usize;
                    if dst < sources.len() {
                        sources[dst] = if src < sources.len() {
                            sources[src]
                        } else {
                            None
                        };
                    }
                    ip += 2;
                }
                OpCode::SetFixedField => {
                    let this_r = (code[ip] >> 8) as usize;
                    if this_r >= sources.len() || sources[this_r] != Some(RegSource::This) {
                        return None;
                    }
                    let val_r = (code[ip + 1] >> 8) as usize;
                    let tag = (code[ip + 1] & 0xFF) as u8;
                    // A compact class field; `w3` is its baked byte offset.
                    let varn_core::FieldAccess::Compact(kind) = varn_core::FieldAccess::decode(tag)
                    else {
                        return None;
                    };
                    let offset = code[ip + 3] as u32;
                    if val_r >= sources.len() {
                        return None;
                    }
                    let Some(RegSource::Param(param_idx)) = sources[val_r] else {
                        return None;
                    };
                    plan.push((param_idx, offset, kind));
                    ip += 4;
                }
                OpCode::LoadNull => {
                    let dst = (code[ip] >> 8) as usize;
                    if dst < sources.len() {
                        sources[dst] = None;
                    }
                    ip += 1;
                }
                OpCode::Return => {
                    return Some(plan);
                }
                OpCode::Nop => {
                    ip += 1;
                }
                _ => return None,
            }
        }
        Some(plan)
    }
}
