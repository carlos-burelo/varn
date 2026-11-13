use cranelift_codegen::ir::{
    types, ExtFuncData, ExternalName, Function, InstBuilder, MemFlagsData, UserExternalName,
    UserFuncName,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

use super::super::native_abi::{NativeClass, NativeShape};
use super::super::piece::{compile_piece, CompiledPiece};
use super::setup::Reach;
use crate::JitHelpers;
use rustc_hash::FxHashSet;
use vn_types::ssa::{SsaOp, SsaProto};

pub(super) fn build(
    shape: &NativeShape,
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
) -> Result<CompiledPiece, String> {
    let mut func = Function::with_name_signature(UserFuncName::user(0, 2), shape.signature());
    let body_sig = func.import_signature(shape.self_signature());
    let body_name = func.declare_imported_user_function(UserExternalName::new(0, 0));
    let body_ref = func.import_function(ExtFuncData {
        name: ExternalName::user(body_name),
        signature: body_sig,
        colocated: true,
        patchable: false,
    });

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);

    let params = b.block_params(block).to_vec();
    let exec_ctx = params[0];
    let first_arg = 2 + NativeClass::of(shape.params[0]).words();
    let layout = &helpers.call_layout;
    let m = MemFlagsData::trusted();
    let frames = b
        .ins()
        .load(types::I64, m, exec_ctx, layout.frames_len_off as i32);
    let frameless = b
        .ins()
        .load(types::I64, m, exec_ctx, layout.frameless_depth_off as i32);
    let depth = b.ins().iadd(frames, frameless);
    let mut args = vec![exec_ctx, depth];
    args.extend_from_slice(&params[first_arg..]);
    b.ins().return_call(body_ref, &args);
    b.finalize(isa.frontend_config());
    compile_piece(func, isa)
}

pub(super) fn redundant_guards(ssa: &SsaProto, reach: &Reach) -> FxHashSet<u32> {
    let entry = ssa.entry as usize;
    let calls_self = |blk: &vn_types::ssa::SsaBlock| {
        blk.insts
            .iter()
            .any(|i| matches!(i.op, SsaOp::SelfCall { .. }))
    };
    let has_call: Vec<bool> = ssa.blocks.iter().map(calls_self).collect();
    let mut guarded_in = vec![true; ssa.blocks.len()];
    guarded_in[entry] = false;
    loop {
        let mut changed = false;
        for &blk in &reach.rpo {
            if blk == entry {
                continue;
            }
            let guarded = reach.preds[blk]
                .iter()
                .filter(|p| reach.reached[**p])
                .all(|&p| guarded_in[p] || has_call[p]);
            if guarded != guarded_in[blk] {
                guarded_in[blk] = guarded;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut redundant = FxHashSet::default();
    for &blk in &reach.rpo {
        let mut guarded = guarded_in[blk];
        for inst in &ssa.blocks[blk].insts {
            if matches!(inst.op, SsaOp::SelfCall { .. }) {
                if guarded {
                    redundant.insert(inst.ip);
                }
                guarded = true;
            }
        }
    }
    redundant
}
