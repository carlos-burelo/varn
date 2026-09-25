//! The layout table (`varn_types::bytecode::layout`) describes the bytecode
//! the compiler really emits: checked over every program of the test suite.
//!
//! For each instruction of every function: the stream is walked exactly
//! (lengths add up to the code), every register operand is a register of the
//! frame, every constant operand names a pool entry of the kind the table
//! says, every jump lands on an instruction, the listing decodes it, and
//! renaming registers touches exactly the bytes the table calls registers.

#![allow(unused_crate_dependencies)]

use std::collections::BTreeSet;

use varn_core::OpCode;
use varn_types::bytecode::{self, disasm, layout, At, ConstKind, Operand};
use varn_types::{FunctionProto, Literal, PoolEntry};

fn suite() -> Vec<(String, FunctionProto)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "vn"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).ok()?;
            let name = path.to_string_lossy().to_string();
            let proto = varn_pipeline::stdlib_loader::compile_source(&source, &name).ok()?;
            Some((name, proto))
        })
        .collect()
}

fn functions<'a>(proto: &'a FunctionProto, out: &mut Vec<&'a FunctionProto>) {
    out.push(proto);
    for c in &proto.chunk.constants {
        if let PoolEntry::Function(f) = c {
            functions(f, out);
        }
    }
}

fn const_matches(kind: ConstKind, entry: &PoolEntry) -> bool {
    matches!(
        (kind, entry),
        (ConstKind::Value, PoolEntry::Literal(_))
            | (
                ConstKind::Name | ConstKind::Module,
                PoolEntry::Literal(Literal::Str(_))
            )
            | (ConstKind::NativeOp, PoolEntry::Literal(Literal::Int(_)))
            | (ConstKind::Symbol, PoolEntry::Literal(Literal::Symbol(_)))
            | (ConstKind::Function, PoolEntry::Function(_))
            | (ConstKind::Shape, PoolEntry::Shape(_))
    )
}

/// Check one function; returns the opcodes it uses.
fn check(file: &str, proto: &FunctionProto, seen: &mut BTreeSet<String>) {
    let name = proto.name.as_deref().unwrap_or("<anonymous>");
    let at = |offset: usize| format!("{file} fn {name} @{offset:04}");
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;

    let mut starts = BTreeSet::new();
    let mut register_bytes = BTreeSet::new();
    let mut offset = 0;
    while offset < code.len() {
        let l =
            layout(code, offset, pool).unwrap_or_else(|| panic!("{}: not an opcode", at(offset)));
        seen.insert(format!("{:?}", l.op));
        starts.insert(offset);
        assert!(
            offset + l.len <= code.len(),
            "{}: {:?} runs past the code",
            at(offset),
            l.op
        );
        for operand in &l.operands {
            match *operand {
                Operand::Reg { at: b, .. } => {
                    let reg = b.read(code, offset) as u16;
                    // `MakeClass` names `r0` for "no superclass".
                    assert!(
                        reg < proto.register_count.max(1),
                        "{}: {:?} r{reg}",
                        at(offset),
                        l.op
                    );
                    register_bytes.insert((offset + b.word, b.half));
                }
                Operand::Run { start, count, .. } => {
                    let first = start.read(code, offset) as usize;
                    assert!(
                        count == 0 || first + count <= proto.register_count as usize,
                        "{}: {:?} run r{first}+{count} of {}",
                        at(offset),
                        l.op,
                        proto.register_count
                    );
                    register_bytes.insert((offset + start.word, start.half));
                }
                Operand::Const { word, kind } => {
                    let i = At::Word(word).read(code, offset) as usize;
                    let entry = pool
                        .get(i)
                        .unwrap_or_else(|| panic!("{}: const #{i}", at(offset)));
                    assert!(
                        const_matches(kind, entry),
                        "{}: {:?} expects a {kind:?} at #{i}, found {entry:?}",
                        at(offset),
                        l.op
                    );
                }
                Operand::Fixed { .. } | Operand::Imm { .. } | Operand::Jump { .. } => {}
            }
        }
        offset += l.len;
    }
    assert_eq!(
        offset,
        code.len(),
        "{file} fn {name}: lengths do not add up"
    );

    // Jumps land on instructions.
    for &start in &starts {
        let l = layout(code, start, pool).unwrap();
        if let Some(target) = l.jump_target(code, start) {
            assert!(
                target == code.len() || starts.contains(&target),
                "{}: {:?} → {target} is mid-instruction",
                at(start),
                l.op
            );
        }
    }

    // The listing decodes every instruction.
    assert!(disasm::instructions(&proto.chunk).all(|i| i.op.is_some()));

    // Renaming touches exactly the register bytes, and undoes.
    let flip = |r: u8| r ^ 0x5a;
    let mut renamed = code.clone();
    bytecode::remap_registers(&mut renamed, pool, flip);
    for (i, (&old, &new)) in code.iter().zip(&renamed).enumerate() {
        for (half, shift) in [(bytecode::Half::Hi, 8), (bytecode::Half::Lo, 0)] {
            let (o, n) = ((old >> shift) as u8, (new >> shift) as u8);
            if register_bytes.contains(&(i, half)) {
                assert_eq!(
                    n,
                    flip(o),
                    "{file} fn {name}: word {i} {half:?} not renamed"
                );
            } else {
                assert_eq!(
                    n, o,
                    "{file} fn {name}: word {i} {half:?} is not a register"
                );
            }
        }
    }
    bytecode::remap_registers(&mut renamed, pool, flip);
    assert_eq!(&renamed, code, "{file} fn {name}: renaming does not undo");
}

#[test]
fn the_layout_table_describes_the_emitted_bytecode() {
    let programs = suite();
    assert!(
        programs.len() > 100,
        "only {} test programs compiled",
        programs.len()
    );
    let mut seen = BTreeSet::new();
    for (file, module) in &programs {
        let mut fns = Vec::new();
        functions(module, &mut fns);
        for f in fns {
            check(file, f, &mut seen);
        }
    }
    let unseen: Vec<String> = (0..=u8::MAX)
        .filter_map(OpCode::from_u8)
        .map(|op| format!("{op:?}"))
        .filter(|op| !seen.contains(op))
        .collect();
    eprintln!(
        "{} programs, {} opcodes seen; not emitted by the suite: {unseen:?}",
        programs.len(),
        seen.len()
    );
}
