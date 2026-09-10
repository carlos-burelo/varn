//! Lowering invariants Cranelift's own verifier cannot express.
//!
//! Every rule here failed in production at least once, and in each case the
//! emitted IR was perfectly well formed — the verifier had nothing to say. The
//! damage showed up as a segfault or as silently wrong values, hours of reading
//! `-p clif:ir` by eye away from the instruction that caused it.
//!
//! A violation is a compiler bug, never a program bug. `vn debug -p clif:check`
//! reports them; the lowering itself only records them, so a broken invariant
//! can be inspected rather than turning every compile into a panic.

use cranelift_codegen::ir::{instructions::InstructionData, types, Function, Inst, Opcode, Value};

/// One violated invariant, addressed to whoever is reading `-p clif:check`.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Short stable name of the rule, for grepping across a corpus sweep.
    pub rule: &'static str,
    /// What is wrong, in terms of the instruction that is wrong.
    pub detail: String,
}

/// What the checker needs to know about the lowering that produced `func`.
pub struct Context {
    pub frame_aware: bool,
    /// The entry block's `base` parameter, present only when frame-aware. A
    /// direct self-call must never forward it.
    pub caller_base: Option<Value>,
    /// The placeholder a leaf lowering gets where a frame-aware one would have
    /// `exec_ctx`. Present only when the lowering is a leaf.
    pub leaf_ctx: Option<Value>,
}

/// Check every rule against a finished function. Cheap enough to run on every
/// lowering: one pass over the instructions, no allocation unless something is
/// actually wrong.
pub fn check(func: &Function, ctx: &Context) -> Vec<Violation> {
    let mut out = Vec::new();
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            leaf_ctx_dereferenced(func, inst, ctx, &mut out);
            self_call_forwards_caller_frame(func, inst, ctx, &mut out);
            call_arg_types_match_signature(func, inst, &mut out);
            stack_store_of_pair(func, inst, &mut out);
        }
    }
    out.dedup_by(|a, b| a.rule == b.rule && a.detail == b.detail);
    out
}

/// A leaf lowering dereferencing the placeholder that stands in for `exec_ctx`.
///
/// Reading the heap means loading its base off `exec_ctx`, and the leaf calling
/// convention does not carry one — it passes a placeholder zero. A lowering
/// that walks the heap without asking to be frame-aware turns that into
/// `load [0 + 144]`, a null dereference the moment the function runs. It cost a
/// segfault in `tests/09-control-flow.vn` and hours of reading IR by eye.
///
/// The rule names the placeholder rather than testing for a zero address,
/// because a zero address is not by itself wrong: the array and object caches
/// deliberately zero their base and guard every use with `icmp_imm ne base, 0`,
/// so a load off a constant zero in a block the guard dominates is correct and
/// ordinary. Only the placeholder is unguarded by construction.
fn leaf_ctx_dereferenced(func: &Function, inst: Inst, ctx: &Context, out: &mut Vec<Violation>) {
    let Some(placeholder) = ctx.leaf_ctx else {
        return;
    };
    let opcode = func.dfg.insts[inst].opcode();
    let addr = match opcode {
        Opcode::Load
        | Opcode::Uload8
        | Opcode::Sload8
        | Opcode::Uload16
        | Opcode::Sload16
        | Opcode::Uload32
        | Opcode::Sload32 => func.dfg.inst_args(inst).first().copied(),
        Opcode::Store | Opcode::Istore8 | Opcode::Istore16 | Opcode::Istore32 => {
            func.dfg.inst_args(inst).get(1).copied()
        }
        _ => None,
    };
    if addr != Some(placeholder) {
        return;
    }
    out.push(Violation {
        rule: "leaf-ctx-dereferenced",
        detail: format!(
            "{opcode} addresses {placeholder}, the exec_ctx placeholder of a leaf lowering — this function walks the heap and must be lowered frame-aware"
        ),
    });
}

/// A direct self-call handing the callee the caller's own `base`.
///
/// A frame-aware lowering mirrors its registers into `stack[base + r]`. Passing
/// its own `base` to a recursive call points the callee at those same slots, so
/// the callee's writes land on the caller's live values and the caller reads
/// them back after the call. Nothing crashes; the values are simply wrong.
/// Recursion out of a frame-aware lowering goes through `clif_call_self`, which
/// pushes a frame of its own.
fn self_call_forwards_caller_frame(
    func: &Function,
    inst: Inst,
    ctx: &Context,
    out: &mut Vec<Violation>,
) {
    let Some(base) = ctx.caller_base else { return };
    if !ctx.frame_aware {
        return;
    }
    let InstructionData::Call { func_ref, .. } = func.dfg.insts[inst] else {
        return;
    };
    // The raw body imports exactly one function: itself, for recursion.
    let args = func.dfg.inst_args(inst);
    if args.get(2).copied() != Some(base) {
        return;
    }
    out.push(Violation {
        rule: "self-call-forwards-frame",
        detail: format!(
            "call {func_ref} passes the caller's own base {base} as the callee's \
             frame base — the callee would overwrite the caller's home slots"
        ),
    });
}

/// A call whose argument types disagree with the signature it is made against.
///
/// The raw calling convention is decided in one place and consumed in three
/// (the body, its wrapper, and every direct call site). When those drift, the
/// callee reads whatever the mismatched register happened to hold.
fn call_arg_types_match_signature(func: &Function, inst: Inst, out: &mut Vec<Violation>) {
    let (sig_ref, skip) = match func.dfg.insts[inst] {
        InstructionData::CallIndirect { sig_ref, .. } => (sig_ref, 1),
        InstructionData::Call { func_ref, .. } => (func.dfg.ext_funcs[func_ref].signature, 0),
        _ => return,
    };
    let sig = &func.dfg.signatures[sig_ref];
    let args = &func.dfg.inst_args(inst)[skip..];
    if args.len() != sig.params.len() {
        out.push(Violation {
            rule: "call-arity",
            detail: format!(
                "call passes {} argument(s) to a signature taking {}",
                args.len(),
                sig.params.len()
            ),
        });
        return;
    }
    for (i, (&arg, param)) in args.iter().zip(&sig.params).enumerate() {
        let got = func.dfg.value_type(arg);
        if got != param.value_type {
            out.push(Violation {
                rule: "call-arg-type",
                detail: format!(
                    "call argument {i} is {got}, signature declares {}",
                    param.value_type
                ),
            });
        }
    }
}

/// A `VmValue` spilled to a stack slot as one 128-bit store.
///
/// A `VmValue` is a tag and a payload, and the rest of the lowering addresses
/// the two halves separately. Storing it whole leaves the halves unreachable to
/// every reader that expects the pair.
fn stack_store_of_pair(func: &Function, inst: Inst, out: &mut Vec<Violation>) {
    let InstructionData::StackStore { arg, .. } = func.dfg.insts[inst] else {
        return;
    };
    if func.dfg.value_type(arg) != types::I128 {
        return;
    }
    out.push(Violation {
        rule: "stack-store-pair",
        detail: format!("stack_store writes {arg} as one i128 instead of a tag/payload pair"),
    });
}
