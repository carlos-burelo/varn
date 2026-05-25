use super::super::compiler::Compiler;
use varn_core::OpCode;

use super::compile_expr;

pub fn emit_field_inits<'a>(c: &mut Compiler<'a>) {
    if c.pending_field_inits.is_empty() {
        return;
    }
    let inits = std::mem::take(&mut c.pending_field_inits);
    for (name, expr) in inits {
        let this_r = 0u8;
        let val = compile_expr(c, &expr);
        let key_idx = c.add_str(&name);
        c.emit_property(OpCode::SetProperty, this_r, val, key_idx);
        c.free_reg();
    }
}
