use varn_core::OpCode;

use super::definition::FunctionProto;

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

    pub fn resumes_in_interpreter(&self) -> bool {
        match self.resume_memo.get() {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let code = &self.chunk.code;
        let mut ip = 0;
        let mut found = false;
        while ip < code.len() {
            let Some(info) = crate::bytecode::decode(code, ip, &self.chunk.constants) else {
                found = true;
                break;
            };
            if matches!(
                OpCode::from_u16(code[ip]),
                Some(OpCode::Try | OpCode::Yield | OpCode::Await | OpCode::LoadModule)
            ) {
                found = true;
                break;
            }
            ip += info.len.max(1);
        }
        self.resume_memo.set(if found { 1 } else { 2 });
        found
    }
}
