use varn_core::OpCode;

use super::definition::FunctionProto;

impl FunctionProto {
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
}
