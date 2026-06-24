use varn_core::OpCode;
use varn_types::chunk::FunctionProto;
use varn_types::register_meta::{RegisterMeta, SlotKind};

pub fn infer(proto: &mut FunctionProto) {
    let n = proto.register_count as usize;
    if n == 0 {
        return;
    }

    let mut kinds: Vec<SlotKind> = vec![SlotKind::Dynamic; n];
    let mut tainted: Vec<bool> = vec![false; n];
    let code = &proto.chunk.code;
    let mut ip = 0;

    while ip < code.len() {
        let raw = code[ip];
        ip += 1;
        let dst = (raw >> 8) as usize;
        let op = match OpCode::from_u8(raw as u8) {
            Some(o) => o,
            None => continue,
        };

        match op {
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::StrConcat => {
                if dst < n {
                    tainted[dst] = true;
                }
                ip += 1;
            }
            OpCode::LoadIntZero | OpCode::LoadIntOne | OpCode::LoadIntMinusOne => {
                set_if_dominant(&mut kinds, dst, SlotKind::Int);
            }
            OpCode::LoadInt => {
                set_if_dominant(&mut kinds, dst, SlotKind::Int);
                ip += 1;
            }
            OpCode::LoadTrue | OpCode::LoadFalse => {
                set_if_dominant(&mut kinds, dst, SlotKind::Bool);
            }
            OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt => {
                set_if_dominant(&mut kinds, dst, SlotKind::Int);
                ip += 1;
            }
            OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat => {
                set_if_dominant(&mut kinds, dst, SlotKind::Float);
                ip += 1;
            }
            OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat => {
                set_if_dominant(&mut kinds, dst, SlotKind::Bool);
                ip += 1;
            }
            OpCode::AddImm | OpCode::SubImm => {
                ip += 1;
            }
            _ => {
                ip += instruction_extra_words(op, code, ip);
            }
        }
    }

    for (i, kind) in kinds.iter_mut().enumerate() {
        if tainted[i] {
            *kind = SlotKind::Dynamic;
        }
    }

    proto.register_meta = kinds
        .into_iter()
        .map(|kind| RegisterMeta { kind })
        .collect();
}

#[inline]
fn set_if_dominant(kinds: &mut [SlotKind], dst: usize, kind: SlotKind) {
    if dst < kinds.len() && kinds[dst] == SlotKind::Dynamic {
        kinds[dst] = kind;
    }
}

fn instruction_extra_words(op: OpCode, code: &[u16], ip: usize) -> usize {
    use OpCode::*;
    match op {
        LoadConst | Move | LoadGlobal | StoreGlobal | DefineGlobal | LoadGlobalIdx
        | StoreGlobalIdx | DefineGlobalIdx | LoadUpvalue | StoreUpvalue | CloseUpvalue | Add
        | Sub | Mul | Div | Mod | Pow | Negate | Not | ToString | Eq | Neq | Lt | Lte | Gt
        | Gte | BitAnd | BitOr | BitXor | Shl | Shr | Ushr | StrConcat | StrLength | StrSlice
        | ArrayLength | ArrayPush | ArrayPop | GetIndex | SetIndex | GetEnumTag | IsNull
        | IsArray | AssertNotNull | Typeof | Instanceof | In | WrapSpread | Yield | Await
        | Spawn | Throw | Return | GetFixedField | SetFixedField | GetSuper | GetSymbol
        | BindMethod | Nop => 1,

        LoadNull | LoadTrue | LoadFalse | LoadIntZero | LoadIntOne | LoadIntMinusOne | PopTry => 0,

        Jump | Loop | JumpIfFalse | JumpIfTrue => 1,

        Call | CallMethod | InvokeVirtual => {
            if ip < code.len() {
                let w = code[ip];
                let arg_count = (w >> 8) as usize;
                2 + arg_count.saturating_sub(1)
            } else {
                1
            }
        }

        _ => 1,
    }
}
