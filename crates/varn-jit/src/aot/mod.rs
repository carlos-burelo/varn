use cranelift_codegen::ir::{types, AbiParam, InstBuilder, UserFuncName};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use rustc_hash::FxHashMap as HashMap;
use varn_types::chunk::{Literal, PoolEntry};

pub struct AotOutput {
    pub object_bytes: Vec<u8>,
}

pub fn compile_to_object(
    proto: &varn_types::FunctionProto,
    isa: &OwnedTargetIsa,
) -> Result<AotOutput, String> {
    let obj_builder = ObjectBuilder::new(isa.clone(), "varn_aot_module", default_libcall_names())
        .map_err(|e| format!("aot: ObjectBuilder: {e}"))?;
    let mut module = ObjectModule::new(obj_builder);

    let rt_helpers = declare_rt_helpers(&mut module)?;

    let main_sig = {
        let mut sig = module.make_signature();

        sig.returns.push(AbiParam::new(types::I64));
        sig
    };
    let main_id = module
        .declare_function("_varn_main", Linkage::Export, &main_sig)
        .map_err(|e| format!("aot: declare _varn_main: {e}"))?;

    let mut ctx = module.make_context();
    ctx.func.signature = main_sig;
    ctx.func.name = UserFuncName::user(0, 0);

    let mut fb_ctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        emit_module_body(&mut builder, &mut module, main_id, proto, &rt_helpers)?;

        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().return_(&[zero]);
        builder.finalize(module.isa().frontend_config());
    }

    module
        .define_function(main_id, &mut ctx)
        .map_err(|e| format!("aot: define _varn_main: {e}"))?;

    let product = module.finish();
    let bytes = product.emit().map_err(|e| format!("aot: emit: {e}"))?;

    Ok(AotOutput {
        object_bytes: bytes,
    })
}

struct RtHelpers {
    print: FuncId,
    str_concat: FuncId,
}

fn declare_rt_helpers(module: &mut ObjectModule) -> Result<RtHelpers, String> {
    let map_err = |e: cranelift_module::ModuleError| format!("aot: declare helper: {e}");

    let mut print_sig = module.make_signature();
    print_sig.params.push(AbiParam::new(types::I64));
    print_sig.params.push(AbiParam::new(types::I64));
    let print = module
        .declare_function("varn_rt_print", Linkage::Import, &print_sig)
        .map_err(map_err)?;

    let mut sc_sig = module.make_signature();
    sc_sig.params.push(AbiParam::new(types::I64));
    sc_sig.params.push(AbiParam::new(types::I64));
    sc_sig.params.push(AbiParam::new(types::I64));
    sc_sig.params.push(AbiParam::new(types::I64));
    sc_sig.returns.push(AbiParam::new(types::I64));
    sc_sig.returns.push(AbiParam::new(types::I64));
    let str_concat = module
        .declare_function("varn_rt_str_concat", Linkage::Import, &sc_sig)
        .map_err(map_err)?;

    Ok(RtHelpers { print, str_concat })
}

fn emit_module_body(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    _self_id: FuncId,
    proto: &varn_types::FunctionProto,
    rt: &RtHelpers,
) -> Result<(), String> {
    use varn_core::OpCode;
    use varn_types::bytecode::decode;

    let code = &proto.chunk.code;
    let constants = &proto.chunk.constants;
    let nregs = proto.register_count as usize;

    let mut vars = Vec::with_capacity(nregs * 2);
    for _ in 0..nregs {
        let v_val = builder.declare_var(types::I64);
        let v_meta = builder.declare_var(types::I64);
        vars.push((v_val, v_meta));

        let z = builder.ins().iconst(types::I64, 0);
        builder.def_var(v_val, z);
        builder.def_var(v_meta, z);
    }

    let mut data_ids: HashMap<usize, cranelift_module::DataId> = HashMap::default();

    let mut ip = 0usize;
    while ip < code.len() {
        let Some(info) = decode(code, ip, constants) else {
            break;
        };

        let w0 = code[ip];
        let first_reg = (w0 >> 8) as usize;
        let next_ip = ip + info.len;

        let Some(op) = OpCode::from_u8((w0 & 0xFF) as u8) else {
            ip = next_ip;
            continue;
        };

        match op {
            OpCode::LoadInt => {
                let val = code[ip + 1] as i16 as i64;
                let c = builder.ins().iconst(types::I64, val);
                builder.def_var(vars[first_reg].0, c);
                let tag = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::LoadIntZero => {
                let z = builder.ins().iconst(types::I64, 0);
                builder.def_var(vars[first_reg].0, z);
                let tag = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::LoadIntOne => {
                let one = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].0, one);
                let tag = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::LoadConst => {
                let cidx = code[ip + 1] as usize;
                if let Some(entry) = constants.get(cidx) {
                    match entry {
                        PoolEntry::Literal(Literal::Str(s)) => {
                            let data_id = get_or_create_string_data(
                                module,
                                &mut data_ids,
                                cidx,
                                s.as_bytes(),
                            )?;
                            let gv = module.declare_data_in_func(data_id, builder.func);
                            let ptr = builder.ins().symbol_value(types::I64, gv);
                            builder.def_var(vars[first_reg].0, ptr);
                            let len = builder.ins().iconst(types::I64, s.len() as i64);
                            builder.def_var(vars[first_reg].1, len);
                        }
                        PoolEntry::Literal(Literal::Int(val)) => {
                            let c = builder.ins().iconst(types::I64, *val);
                            builder.def_var(vars[first_reg].0, c);
                            let tag = builder.ins().iconst(types::I64, 1);
                            builder.def_var(vars[first_reg].1, tag);
                        }
                        PoolEntry::Literal(Literal::Bool(b)) => {
                            let val = if *b { 1 } else { 0 };
                            let c = builder.ins().iconst(types::I64, val);
                            builder.def_var(vars[first_reg].0, c);
                            let tag = builder.ins().iconst(types::I64, 2);
                            builder.def_var(vars[first_reg].1, tag);
                        }
                        _ => {
                            let z = builder.ins().iconst(types::I64, 0);
                            builder.def_var(vars[first_reg].0, z);
                            builder.def_var(vars[first_reg].1, z);
                        }
                    }
                }
            }
            OpCode::LoadTrue => {
                let one = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].0, one);
                let tag = builder.ins().iconst(types::I64, 2);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::LoadFalse => {
                let z = builder.ins().iconst(types::I64, 0);
                builder.def_var(vars[first_reg].0, z);
                let tag = builder.ins().iconst(types::I64, 2);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::LoadNull => {
                let z = builder.ins().iconst(types::I64, 0);
                builder.def_var(vars[first_reg].0, z);
                builder.def_var(vars[first_reg].1, z);
            }
            OpCode::Move => {
                let w1 = code[ip + 1];
                let src = (w1 >> 8) as usize;
                let val = builder.use_var(vars[src].0);
                let meta = builder.use_var(vars[src].1);
                builder.def_var(vars[first_reg].0, val);
                builder.def_var(vars[first_reg].1, meta);
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Mod => {
                let w1 = code[ip + 1];
                let lhs_r = (w1 >> 8) as usize;
                let rhs_r = (w1 & 0xFF) as usize;
                let lhs = builder.use_var(vars[lhs_r].0);
                let rhs = builder.use_var(vars[rhs_r].0);
                let res = match op {
                    OpCode::Add => builder.ins().iadd(lhs, rhs),
                    OpCode::Sub => builder.ins().isub(lhs, rhs),
                    OpCode::Mul => builder.ins().imul(lhs, rhs),
                    OpCode::Mod => builder.ins().srem(lhs, rhs),
                    _ => unreachable!(),
                };
                builder.def_var(vars[first_reg].0, res);
                let tag = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::AddImm => {
                let w1 = code[ip + 1];
                let src_r = (w1 >> 8) as usize;
                let imm = (w1 & 0xFF) as i8 as i64;
                let src = builder.use_var(vars[src_r].0);
                let res = builder.ins().iadd_imm_u(src, imm);
                builder.def_var(vars[first_reg].0, res);
                let tag = builder.ins().iconst(types::I64, 1);
                builder.def_var(vars[first_reg].1, tag);
            }
            OpCode::StrConcat => {
                let w1 = code[ip + 1];
                let lhs_r = (w1 >> 8) as usize;
                let rhs_r = (w1 & 0xFF) as usize;

                let a_ptr = builder.use_var(vars[lhs_r].0);
                let a_len = builder.use_var(vars[lhs_r].1);
                let b_ptr = builder.use_var(vars[rhs_r].0);
                let b_len = builder.use_var(vars[rhs_r].1);

                let fref = module.declare_func_in_func(rt.str_concat, builder.func);
                let call = builder.ins().call(fref, &[a_ptr, a_len, b_ptr, b_len]);
                let results = builder.inst_results(call);
                let res_ptr = results[0];
                let res_len = results[1];
                builder.def_var(vars[first_reg].0, res_ptr);
                builder.def_var(vars[first_reg].1, res_len);
            }
            OpCode::Call => {
                let w2 = code[ip + 2];
                let argc = (w2 >> 8) as usize;
                let arg_start = (w2 & 0xFF) as usize;

                if argc == 2 {
                    let arg_r = arg_start + 1;
                    let val = builder.use_var(vars[arg_r].0);
                    let meta = builder.use_var(vars[arg_r].1);

                    let fref_print = module.declare_func_in_func(rt.print, builder.func);
                    builder.ins().call(fref_print, &[val, meta]);
                }
            }
            OpCode::Return => {
                break;
            }
            _ => {}
        }

        ip = next_ip;
    }

    Ok(())
}

fn get_or_create_string_data(
    module: &mut ObjectModule,
    cache: &mut HashMap<usize, cranelift_module::DataId>,
    idx: usize,
    bytes: &[u8],
) -> Result<cranelift_module::DataId, String> {
    if let Some(&id) = cache.get(&idx) {
        return Ok(id);
    }
    let name = format!(".str.{idx}");
    let data_id = module
        .declare_data(&name, Linkage::Local, false, false)
        .map_err(|e| format!("aot: declare data: {e}"))?;

    let mut desc = cranelift_module::DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());

    module
        .define_data(data_id, &desc)
        .map_err(|e| format!("aot: define data: {e}"))?;

    cache.insert(idx, data_id);
    Ok(data_id)
}
