use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_core::intrinsic_ops::math::MathOp;
use varn_core::ValueConv;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::SsaProto;

use super::super::emit::box_f64;
use super::{call, heap, load_value, Ctx, Out};

fn exec_ctx(ctx: &Ctx<'_>) -> Value {
    heap::exec_ctx(ctx)
}

fn native_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, ectx: Value) -> Value {
    b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}

fn float_instruction(b: &mut FunctionBuilder, ctx: &Ctx<'_>, wire: u8, x: Value) -> Option<Value> {
    Some(match wire {
        w if w == MathOp::Abs as u8 => b.ins().fabs(x),
        w if w == MathOp::Sqrt as u8 => b.ins().sqrt(x),
        w if w == MathOp::Floor as u8 && ctx.has_round => b.ins().floor(x),
        w if w == MathOp::Ceil as u8 && ctx.has_round => b.ins().ceil(x),
        _ => return None,
    })
}

pub(super) fn emit_intrinsic(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    args: &[u32],
    wire: u8,
    dest: Option<u32>,
) -> Result<Out, String> {
    if let [arg] = args
        && ctx.ssa.value_ty(*arg) == SlotKind::Float
    {
        let x = load_value(b, ctx, values, *arg)?;
        if let Some(r) = float_instruction(b, ctx, wire, x) {
            let float_dest = dest.is_some_and(|d| ctx.ssa.value_ty(d) == SlotKind::Float);
            return Ok(if float_dest {
                Out::Native(r)
            } else {
                Out::Boxed(box_f64(b, r))
            });
        }
    }
    let ectx = exec_ctx(ctx);
    let receiver = heap::boxed_value(b, ctx, values, object)?;
    let window = call::boxed_window(b, ctx, values, receiver, args)?;
    let wire_v = b.ins().iconst(types::I64, i64::from(wire));
    let count = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    super::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.intrinsic_window,
        &[ectx, wire_v, window, count],
    )?;
    Ok(Out::Boxed(native_result(b, ctx, ectx)))
}

pub(super) fn is_inline_convert(ssa: &SsaProto, operand: u32, conv: ValueConv) -> bool {
    conv == ValueConv::IntToFloat && ssa.value_ty(operand) == SlotKind::Int
}

pub(super) fn emit_convert(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
    conv: ValueConv,
) -> Result<Out, String> {
    if is_inline_convert(ctx.ssa, operand, conv) {
        let a = load_value(b, ctx, values, operand)?;
        return Ok(Out::Native(b.ins().fcvt_from_sint(types::F64, a)));
    }
    let boxed = heap::boxed_value(b, ctx, values, operand)?;
    Ok(Out::Boxed(convert_boxed(b, ctx, boxed, conv)?))
}

pub(super) fn convert_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    boxed: Value,
    conv: ValueConv,
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let (tag, payload) = b.ins().isplit(boxed);
    let Some(expected) = Expected::of(conv) else {
        let conv_v = b.ins().iconst(types::I64, conv as i64);
        super::exceptions::call_checked(
            b,
            ctx,
            ctx.helpers.convert,
            &[ectx, conv_v, tag, payload],
        )?;
        return Ok(native_result(b, ctx, ectx));
    };

    let pass = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.set_cold_block(slow);
    b.append_block_param(merge, types::I128);
    let kind = b
        .ins()
        .band_imm_u(tag, varn_types::vm_value::KIND_MASK as i64);
    let after_null = if expected.nullable {
        let is_null = b.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            kind,
            varn_types::vm_value::KIND_NULL as i64,
        );
        let next = b.create_block();
        b.ins().brif(is_null, pass, &[], next, &[]);
        next
    } else {
        let next = b.create_block();
        b.ins().jump(next, &[]);
        next
    };
    b.switch_to_block(after_null);
    match expected.kind {
        ExpectedKind::Str => {
            call::emit_is_str(b, &ctx.helpers.array_layout, tag, payload, pass, slow);
        }
        ExpectedKind::Tag(want) => {
            let hit = b.ins().icmp_imm_u(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                kind,
                want as i64,
            );
            b.ins().brif(hit, pass, &[], slow, &[]);
        }
    }

    b.switch_to_block(pass);
    let same = b.ins().iconcat(tag, payload);
    b.ins().jump(merge, &[same.into()]);

    b.switch_to_block(slow);
    let conv_v = b.ins().iconst(types::I64, conv as i64);
    super::exceptions::call_checked(b, ctx, ctx.helpers.convert, &[ectx, conv_v, tag, payload])?;
    let converted = native_result(b, ctx, ectx);
    b.ins().jump(merge, &[converted.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}

enum ExpectedKind {
    Str,
    Tag(u64),
}

struct Expected {
    kind: ExpectedKind,
    nullable: bool,
}

impl Expected {
    fn of(conv: ValueConv) -> Option<Self> {
        use varn_types::vm_value::{KIND_BOOL, KIND_FLOAT, KIND_INT};
        let (kind, nullable) = match conv {
            ValueConv::ExpectStr => (ExpectedKind::Str, false),
            ValueConv::ExpectStrOrNull => (ExpectedKind::Str, true),
            ValueConv::ExpectBool => (ExpectedKind::Tag(KIND_BOOL), false),
            ValueConv::ExpectBoolOrNull => (ExpectedKind::Tag(KIND_BOOL), true),
            ValueConv::ExpectInt => (ExpectedKind::Tag(KIND_INT), false),
            ValueConv::ExpectIntOrNull => (ExpectedKind::Tag(KIND_INT), true),
            ValueConv::ExpectFloat => (ExpectedKind::Tag(KIND_FLOAT), false),
            ValueConv::ExpectFloatOrNull => (ExpectedKind::Tag(KIND_FLOAT), true),
            ValueConv::ExpectRef => (ExpectedKind::Tag(varn_types::vm_value::KIND_HEAP), true),
            ValueConv::IntToFloat
            | ValueConv::FloatToInt
            | ValueConv::IntToBigInt
            | ValueConv::BigIntToInt
            | ValueConv::IntToDecimal
            | ValueConv::DecimalToInt
            | ValueConv::DynToInt
            | ValueConv::DynToFloat
            | ValueConv::BigIntToFloat
            | ValueConv::FloatToBigInt
            | ValueConv::DecimalToFloat
            | ValueConv::FloatToDecimal
            | ValueConv::BigIntToDecimal
            | ValueConv::DecimalToBigInt => return None,
        };
        Some(Expected { kind, nullable })
    }
}
