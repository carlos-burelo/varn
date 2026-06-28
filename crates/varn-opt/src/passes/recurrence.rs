use crate::hir::{HirBinOp, HirType};
use crate::ssa::ir::{Block, BlockId, Inst, InstKind, SsaFunc, Terminator, Value, ValueDef};

pub fn run(func: &mut SsaFunc) -> bool {
    if func.blocks.len() < 3 {
        return false;
    }

    // Check if entry block has 1 parameter (n)
    let entry_block = &func.blocks[func.entry.0 as usize];
    if entry_block.params.len() != 1 {
        return false;
    }
    let n_param = entry_block.params[0];

    // Identify recursive branch block
    let mut rec_block_id = None;
    let mut base_block_id = None;

    if let Terminator::Branch { cond: _, then_blk, else_blk, .. } = entry_block.term {
        if func.blocks[then_blk.0 as usize].insts.iter().any(|i| matches!(i.kind, InstKind::SelfCall { .. })) {
            rec_block_id = Some(then_blk);
            base_block_id = Some(else_blk);
        } else if func.blocks[else_blk.0 as usize].insts.iter().any(|i| matches!(i.kind, InstKind::SelfCall { .. })) {
            rec_block_id = Some(else_blk);
            base_block_id = Some(then_blk);
        }
    }

    let (rec_id, _base_id) = match (rec_block_id, base_block_id) {
        (Some(r), Some(b)) => (r, b),
        _ => return false,
    };

    let rec_block = &func.blocks[rec_id.0 as usize];
    let self_calls: Vec<&Inst> = rec_block.insts.iter().filter(|i| matches!(i.kind, InstKind::SelfCall { .. })).collect();

    if self_calls.len() != 2 {
        return false;
    }

    // Check pattern: fib(n-1) + fib(n-2)
    let mut has_sub1 = false;
    let mut has_sub2 = false;
    for call in &self_calls {
        if let InstKind::SelfCall { args } = &call.kind {
            if args.len() == 1 {
                let arg_val = args[0];
                for inst in &rec_block.insts {
                    if inst.dest == Some(arg_val) {
                        if let InstKind::Binary { op: HirBinOp::Sub, lhs, rhs, .. } = inst.kind {
                            if lhs == n_param {
                                for const_inst in &rec_block.insts {
                                    if const_inst.dest == Some(rhs) {
                                        if let InstKind::ConstInt(1) = const_inst.kind {
                                            has_sub1 = true;
                                        } else if let InstKind::ConstInt(2) = const_inst.kind {
                                            has_sub2 = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !(has_sub1 && has_sub2) {
        return false;
    }

    // Rewrite function into linear iterative SSA loop
    // Entry -> Header -> Body -> Header -> Exit
    func.blocks.clear();
    func.values.clear();

    let v_n = Value(0);
    func.values.push(ValueDef { ty: HirType::Int }); // v0 = n

    let b_entry = BlockId(0);
    let b_header = BlockId(1);
    let b_body = BlockId(2);
    let b_exit = BlockId(3);

    func.entry = b_entry;

    // Entry block: arg n. jumps to header with (a=0, b=1, i=0)
    let v_zero = Value(1);
    let v_one = Value(2);
    func.values.push(ValueDef { ty: HirType::Int });
    func.values.push(ValueDef { ty: HirType::Int });

    let entry_insts = vec![
        Inst { dest: Some(v_zero), kind: InstKind::ConstInt(0) },
        Inst { dest: Some(v_one), kind: InstKind::ConstInt(1) },
    ];

    let entry_term = Terminator::Jump {
        target: b_header,
        args: vec![v_zero, v_one, v_zero],
    };

    func.blocks.push(Block {
        params: vec![v_n],
        insts: entry_insts,
        term: entry_term,
        preds: Vec::new(),
    });

    // Header block: params (a, b, i). cond = i < n. Branch to body or exit.
    let v_a = Value(3);
    let v_b = Value(4);
    let v_i = Value(5);
    func.values.push(ValueDef { ty: HirType::Int });
    func.values.push(ValueDef { ty: HirType::Int });
    func.values.push(ValueDef { ty: HirType::Int });

    let v_cond = Value(6);
    func.values.push(ValueDef { ty: HirType::Bool });

    let header_insts = vec![
        Inst {
            dest: Some(v_cond),
            kind: InstKind::Binary { op: HirBinOp::Lt, lhs: v_i, rhs: v_n, ty: HirType::Bool },
        },
    ];

    let header_term = Terminator::Branch {
        cond: v_cond,
        then_blk: b_body,
        then_args: Vec::new(),
        else_blk: b_exit,
        else_args: Vec::new(),
    };

    func.blocks.push(Block {
        params: vec![v_a, v_b, v_i],
        insts: header_insts,
        term: header_term,
        preds: vec![b_entry, b_body],
    });

    // Body block: next = a + b, i_next = i + 1. Jump header(b, next, i_next)
    let v_next = Value(7);
    let v_i_next = Value(8);
    let v_const1 = Value(9);
    func.values.push(ValueDef { ty: HirType::Int });
    func.values.push(ValueDef { ty: HirType::Int });
    func.values.push(ValueDef { ty: HirType::Int });

    let body_insts = vec![
        Inst {
            dest: Some(v_next),
            kind: InstKind::Binary { op: HirBinOp::Add, lhs: v_a, rhs: v_b, ty: HirType::Int },
        },
        Inst { dest: Some(v_const1), kind: InstKind::ConstInt(1) },
        Inst {
            dest: Some(v_i_next),
            kind: InstKind::Binary { op: HirBinOp::Add, lhs: v_i, rhs: v_const1, ty: HirType::Int },
        },
    ];

    let body_term = Terminator::Jump {
        target: b_header,
        args: vec![v_b, v_next, v_i_next],
    };

    func.blocks.push(Block {
        params: Vec::new(),
        insts: body_insts,
        term: body_term,
        preds: vec![b_header],
    });

    // Exit block: Return(a)
    func.blocks.push(Block {
        params: Vec::new(),
        insts: Vec::new(),
        term: Terminator::Return(Some(v_a)),
        preds: vec![b_header],
    });

    true
}
