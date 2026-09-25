//! The operand layout of every instruction: which byte or word of it holds
//! what.
//!
//! This is the one description of the instruction encoding outside the
//! emitter and the interpreter. [`super::decode`] (length, defined and used
//! registers, call windows), [`super::remap_registers`] (register
//! renumbering) and [`super::disasm`] (listings) are all read off it, so
//! they cannot disagree with one another; each entry states what the
//! interpreter's handler for that opcode reads (`varn-vm`, `exec/dispatch`).

use varn_core::OpCode;

use super::operand::{Access, At, Byte, ConstKind, Half, ImmKind, Layout, Operand, RunKind};
use crate::chunk::PoolEntry;

const fn hi(word: usize) -> Byte {
    Byte {
        word,
        half: Half::Hi,
    }
}

const fn lo(word: usize) -> Byte {
    Byte {
        word,
        half: Half::Lo,
    }
}

fn r(at: Byte) -> Operand {
    Operand::Reg {
        at,
        access: Access::Read,
    }
}

fn w(at: Byte) -> Operand {
    Operand::Reg {
        at,
        access: Access::Write,
    }
}

fn k(word: usize, kind: ConstKind) -> Operand {
    Operand::Const { word, kind }
}

fn imm(at: Byte, kind: ImmKind) -> Operand {
    Operand::Imm {
        at: At::Byte(at),
        kind,
    }
}

fn imm_word(word: usize, kind: ImmKind) -> Operand {
    Operand::Imm {
        at: At::Word(word),
        kind,
    }
}

fn run(start: Byte, count: usize, kind: RunKind) -> Operand {
    Operand::Run { start, count, kind }
}

fn jump(word: usize, backward: bool) -> Operand {
    Operand::Jump { word, backward }
}

/// The layout of the instruction at `offset`, or `None` when that word is
/// not an opcode. Lengths that depend on the instruction (a closure's
/// captures, an object's pairs) are read from it, and a shape's size from
/// `constants`.
pub fn layout(code: &[u16], offset: usize, constants: &[PoolEntry]) -> Option<Layout> {
    use ConstKind as K;
    use ImmKind as I;
    use OpCode as O;

    let op = OpCode::from_u16(*code.get(offset)?)?;
    let byte = |b: Byte| b.read(code, offset) as usize;
    let word = |i: usize| At::Word(i).read(code, offset) as usize;

    let (len, operands) = match op {
        O::PopTry | O::Nop => (1, vec![]),

        O::LoadNull
        | O::LoadTrue
        | O::LoadFalse
        | O::LoadIntZero
        | O::LoadIntOne
        | O::LoadIntMinusOne => (1, vec![w(hi(0))]),
        O::LoadInt => (2, vec![w(hi(0)), imm_word(1, I::Int)]),
        O::LoadConst => (2, vec![w(hi(0)), k(1, K::Value)]),
        O::LoadStaticFn => (2, vec![w(hi(0)), k(1, K::Function)]),
        O::LoadGlobal => (2, vec![w(hi(0)), k(1, K::Name)]),
        O::LoadGlobalIdx => (2, vec![w(hi(0)), imm_word(1, I::GlobalSlot)]),
        O::LoadNativeGlobalIdx => (2, vec![w(hi(0)), imm_word(1, I::NativeGlobalSlot)]),
        O::StoreGlobal | O::DefineGlobal => (3, vec![r(hi(1)), k(2, K::Name)]),
        O::StoreGlobalIdx | O::DefineGlobalIdx => (3, vec![r(hi(1)), imm_word(2, I::GlobalSlot)]),
        O::LoadUpvalue => (2, vec![w(hi(1)), imm(lo(1), I::Upvalue)]),
        O::StoreUpvalue => (2, vec![imm(hi(1), I::Upvalue), r(lo(1))]),
        // Closes every open upvalue from this register up.
        O::CloseUpvalue => (2, vec![r(hi(1))]),
        O::LoadModule => (2, vec![w(hi(0)), k(1, K::Module)]),
        O::LoadModuleSlot => (3, vec![w(hi(0)), r(hi(1)), imm_word(2, I::ModuleSlot)]),
        O::StoreModuleSlot => (2, vec![r(hi(0)), imm_word(1, I::ModuleSlot)]),

        O::Move
        | O::Negate
        | O::Not
        | O::ToString
        | O::IsNull
        | O::IsArray
        | O::Typeof
        | O::WrapSpread
        | O::ArrayLength
        | O::ArrayPop
        | O::StrLength
        | O::GetEnumTag
        | O::Await
        | O::Spawn
        | O::ObjectKeys => (2, vec![w(hi(0)), r(hi(1))]),
        O::Convert => (2, vec![w(hi(0)), r(hi(1)), imm(lo(1), I::Conv)]),
        O::AddImm | O::SubImm => (2, vec![w(hi(0)), r(hi(1)), imm(lo(1), I::Int)]),
        O::IntrinsicDirect => (2, vec![w(hi(0)), imm(lo(1), I::Intrinsic), r(hi(1))]),

        O::Add
        | O::Sub
        | O::Mul
        | O::Div
        | O::Mod
        | O::Pow
        | O::BitAnd
        | O::BitOr
        | O::BitXor
        | O::Shl
        | O::Shr
        | O::Ushr
        | O::Eq
        | O::Neq
        | O::Lt
        | O::Lte
        | O::Gt
        | O::Gte
        | O::Instanceof
        | O::In
        | O::StrConcat
        | O::StrSlice
        | O::AddInt
        | O::SubInt
        | O::MulInt
        | O::DivInt
        | O::ModInt
        | O::PowInt
        | O::LtInt
        | O::GtInt
        | O::LteInt
        | O::GteInt
        | O::EqInt
        | O::NeqInt
        | O::AddFloat
        | O::SubFloat
        | O::MulFloat
        | O::DivFloat
        | O::ModFloat
        | O::PowFloat
        | O::LtFloat
        | O::GtFloat
        | O::LteFloat
        | O::GteFloat
        | O::EqFloat
        | O::NeqFloat
        | O::GetIndex
        | O::ArrayGetIndex
        | O::MapGetIndex => (2, vec![w(hi(0)), r(hi(1)), r(lo(1))]),
        // Object, index, value.
        O::SetIndex | O::ArraySetIndex | O::MapSetIndex => (2, vec![r(hi(0)), r(hi(1)), r(lo(1))]),
        // The array is mutated in place; its register is not written.
        O::ArrayPush | O::ArrayExtend => (2, vec![r(hi(0)), r(hi(1))]),
        O::ObjectMerge => (
            2,
            vec![
                Operand::Reg {
                    at: hi(0),
                    access: Access::ReadWrite,
                },
                r(hi(1)),
            ],
        ),
        O::AssertNotNull | O::Throw => (2, vec![r(hi(1))]),
        O::Return => (2, vec![r(lo(1))]),
        // The resumed value lands in `hi(1)`.
        O::Yield => (2, vec![w(hi(1)), r(lo(1))]),

        O::Jump => (3, vec![jump(1, false)]),
        O::Loop => (3, vec![jump(1, true)]),
        O::JumpIfFalse | O::JumpIfTrue => (3, vec![r(hi(0)), jump(1, false)]),
        // The error register, then the catch handler.
        O::Try => (4, vec![w(hi(1)), jump(2, false)]),

        O::Call | O::CallSpread => (
            3,
            vec![
                w(hi(1)),
                r(lo(1)),
                run(lo(2), byte(hi(2)), RunKind::CallArgs),
            ],
        ),
        O::CallSelf => (
            3,
            vec![w(hi(1)), run(lo(2), byte(hi(2)), RunKind::CallArgs)],
        ),
        O::CallMethod | O::InvokeVirtual => (
            4,
            vec![
                w(hi(1)),
                r(lo(1)),
                k(2, K::Name),
                run(lo(3), byte(hi(3)), RunKind::CallArgs),
                imm(hi(0), I::CallSite),
            ],
        ),
        // The receiver and arguments are a window from the destination.
        O::Intrinsic => (
            2,
            vec![
                w(hi(0)),
                imm(hi(1), I::Intrinsic),
                run(hi(0), byte(lo(1)), RunKind::CallArgs),
            ],
        ),
        O::CallNativeOp => (
            3,
            vec![
                w(hi(0)),
                k(1, K::NativeOp),
                run(hi(0), word(2), RunKind::CallArgs),
            ],
        ),
        // Only ever a range: `start` and `end`, `flag` for inclusive.
        O::InvokeRuntimeStatic => (
            5,
            vec![
                w(hi(1)),
                k(2, K::Name),
                imm(hi(3), I::Count),
                r(lo(3)),
                r(hi(4)),
                imm(lo(4), I::Flag),
            ],
        ),

        O::GetProperty => (
            3,
            vec![w(hi(0)), r(hi(1)), k(2, K::Name), imm(lo(1), I::CallSite)],
        ),
        O::GetPropertyMaybe => (3, vec![w(hi(0)), r(hi(1)), k(2, K::Name)]),
        O::GetSymbol => (3, vec![w(hi(0)), r(hi(1)), k(2, K::Symbol)]),
        // Object, value.
        O::SetProperty => (
            3,
            vec![r(hi(0)), r(hi(1)), k(2, K::Name), imm(lo(1), I::CallSite)],
        ),
        O::GetFixedField => (
            4,
            vec![w(hi(0)), r(hi(1)), fixed_field(), field_offset(), tag()],
        ),
        O::SetFixedField => (
            4,
            vec![r(hi(0)), r(hi(1)), fixed_field(), field_offset(), tag()],
        ),
        O::GetSuper => (
            2,
            vec![
                w(hi(0)),
                Operand::Fixed {
                    reg: 0,
                    access: Access::Read,
                },
                k(1, K::Name),
            ],
        ),
        O::BindMethod => (3, vec![w(hi(1)), r(lo(1)), k(2, K::Name)]),

        // `hi(1)` is the superclass, `r0` when there is none.
        O::MakeClass => (3, vec![w(hi(0)), k(2, K::Name), r(hi(1))]),
        // Class, superclass.
        O::Inherit => (2, vec![r(hi(1)), r(lo(1))]),
        // Class, function.
        O::Method
        | O::DefineStatic
        | O::DefineGetter
        | O::DefineSetter
        | O::DefineStaticGetter
        | O::DefineStaticSetter => (3, vec![r(hi(1)), k(2, K::Name), r(lo(1))]),
        O::DeclareField => (3, vec![r(hi(1)), k(2, K::Name), imm(lo(1), I::Tag)]),
        // The tag is a register.
        O::MakeEnumVariant => (3, vec![w(hi(1)), k(2, K::Name), r(lo(1))]),

        O::MakeClosure => {
            let captures = byte(lo(1));
            let mut ops = vec![w(hi(1)), k(2, K::Function), imm(lo(1), I::Count)];
            // Each capture is `[is_local][index]`: a register of this frame,
            // or an upvalue of this closure passed down.
            for i in 0..captures {
                ops.push(if byte(hi(3 + i)) == 1 {
                    r(lo(3 + i))
                } else {
                    imm(lo(3 + i), I::Upvalue)
                });
            }
            (3 + captures, ops)
        }
        O::BuildArray | O::BuildTuple => (
            3,
            vec![
                w(hi(1)),
                run(lo(1), byte(hi(2)), RunKind::Values),
                imm(hi(2), I::Count),
            ],
        ),
        // `hi(2)` pairs, keys and values interleaved.
        O::BuildMap => (
            3,
            vec![
                w(hi(1)),
                run(lo(1), 2 * byte(hi(2)), RunKind::Values),
                imm(hi(2), I::Count),
            ],
        ),
        O::BuildObjectWithShape | O::BuildRecord => {
            let fields = match constants.get(word(2)) {
                Some(PoolEntry::Shape(keys)) => keys.len(),
                _ => 0,
            };
            (
                3,
                vec![
                    w(hi(1)),
                    k(2, K::Shape),
                    run(lo(1), fields, RunKind::Values),
                ],
            )
        }
        // `lo(1)` pairs of `[key const][value reg]`.
        O::BuildObject => {
            let pairs = byte(lo(1));
            let mut ops = vec![w(hi(1)), imm(lo(1), I::Count)];
            for i in 0..pairs {
                ops.push(k(2 + 2 * i, K::Name));
                ops.push(r(hi(3 + 2 * i)));
            }
            (2 + 2 * pairs, ops)
        }
        // The keys to leave out follow, one constant each.
        O::ObjectRest => {
            let skipped = byte(hi(2));
            let mut ops = vec![w(hi(1)), r(lo(1)), imm(hi(2), I::Count)];
            ops.extend((0..skipped).map(|i| k(3 + i, K::Name)));
            (3 + skipped, ops)
        }
        // `hi(1)` parts, one register per word.
        O::BuildStr => {
            let parts = byte(hi(1));
            let mut ops = vec![w(hi(0)), imm(hi(1), I::Count)];
            ops.extend((0..parts).map(|i| r(hi(2 + i))));
            (2 + parts, ops)
        }
    };
    Some(Layout { op, len, operands })
}

fn fixed_field() -> Operand {
    imm_word(2, ImmKind::FieldSlot)
}

fn field_offset() -> Operand {
    imm_word(3, ImmKind::FieldOffset)
}

fn tag() -> Operand {
    imm(lo(1), ImmKind::Tag)
}
