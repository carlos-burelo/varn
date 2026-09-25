use varn_core::OpCode;

use super::*;

/// An instruction of `op` with every operand byte set to `fill`, long enough
/// for any layout.
fn instruction(op: OpCode, fill: u8) -> Vec<u16> {
    let word = u16::from_be_bytes([fill, fill]);
    let mut code = vec![word; 64];
    code[0] = ((fill as u16) << 8) | op as u16;
    code
}

fn all_opcodes() -> impl Iterator<Item = OpCode> {
    (0..=u8::MAX).filter_map(OpCode::from_u8)
}

#[test]
fn every_opcode_has_a_layout_within_its_length() {
    for op in all_opcodes() {
        for fill in [0u8, 3] {
            let code = instruction(op, fill);
            let l = layout(&code, 0, &[]).unwrap_or_else(|| panic!("{op:?} has no layout"));
            assert_eq!(l.op, op);
            assert!(l.len >= 1, "{op:?}");
            let inside =
                |word: usize| assert!(word < l.len, "{op:?} names word {word} of {}", l.len);
            for operand in &l.operands {
                match *operand {
                    Operand::Reg { at, .. } | Operand::Run { start: at, .. } => {
                        inside(at.word);
                        assert!(
                            at != Byte {
                                word: 0,
                                half: Half::Lo
                            },
                            "{op:?} names the opcode byte"
                        );
                    }
                    Operand::Const { word, .. } => inside(word),
                    Operand::Imm {
                        at: At::Word(word), ..
                    } => inside(word),
                    Operand::Imm {
                        at: At::Byte(at), ..
                    } => inside(at.word),
                    Operand::Jump { word, .. } => inside(word + 1),
                    Operand::Fixed { .. } => {}
                }
            }
        }
    }
}

#[test]
fn decode_reads_the_layout() {
    for op in all_opcodes() {
        let code = instruction(op, 3);
        let l = layout(&code, 0, &[]).expect("layout");
        let info = decode(&code, 0, &[]).expect("decode");
        assert_eq!(info.len, l.len, "{op:?}");
        assert_eq!(info.opaque, l.opaque(), "{op:?}");
        let writes = l
            .operands
            .iter()
            .filter(|o| matches!(o, Operand::Reg { access, .. } if *access != Access::Read))
            .count();
        assert!(writes <= 1, "{op:?} writes {writes} registers");
        assert_eq!(info.def.is_some(), writes == 1, "{op:?}");
    }
}

/// Operands the interpreter reads and writes that the old hand-written
/// tables got wrong.
#[test]
fn read_write_operands() {
    let merge = decode(&instruction(OpCode::ObjectMerge, 3), 0, &[]).unwrap();
    assert_eq!((merge.def, merge.uses.contains(&3)), (Some(3), true));

    let extend = decode(&instruction(OpCode::ArrayExtend, 3), 0, &[]).unwrap();
    assert_eq!(extend.def, None, "the array is extended in place");

    let yielded = decode(&instruction(OpCode::Yield, 3), 0, &[]).unwrap();
    assert_eq!(
        yielded.def,
        Some(3),
        "the resumed value lands in a register"
    );

    let sup = decode(&instruction(OpCode::GetSuper, 3), 0, &[]).unwrap();
    assert!(sup.uses.contains(&0), "`super` reads `this`");
}

#[test]
fn remap_renames_exactly_the_register_bytes() {
    // `Spawn`: destination in the opcode word, task in the operand's high
    // byte, and a low byte that is not a register.
    let mut code = vec![((4u16) << 8) | OpCode::Spawn as u16, (5u16 << 8) | 9];
    remap_registers(&mut code, &[], |r| r + 10);
    assert_eq!(
        code,
        vec![(14u16 << 8) | OpCode::Spawn as u16, (15u16 << 8) | 9]
    );

    // An `Intrinsic`'s destination is also its window's start: renamed once.
    let mut code = vec![(4u16 << 8) | OpCode::Intrinsic as u16, (0x21u16 << 8) | 2];
    remap_registers(&mut code, &[], |r| r + 1);
    assert_eq!(code[0] >> 8, 5);
    assert_eq!(code[1], (0x21u16 << 8) | 2, "wire byte and count kept");
}

#[test]
fn listing_names_operands() {
    let code = vec![
        (2u16 << 8) | OpCode::AddInt as u16,
        (3u16 << 8) | 4,
        OpCode::Jump as u16,
        0,
        2,
        (1u16 << 8) | OpCode::LoadNull as u16,
    ];
    let l = layout(&code, 0, &[]).unwrap();
    assert_eq!(disasm::operands_text(&l, &code, 0), "r2 = r3 + r4");
    let j = layout(&code, 2, &[]).unwrap();
    assert_eq!(disasm::operands_text(&j, &code, 2), "→ 0007");
    let n = layout(&code, 5, &[]).unwrap();
    assert_eq!(disasm::operands_text(&n, &code, 5), "r1 = null");
}
