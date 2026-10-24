mod emit;
pub mod link;

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, UserFuncName};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

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

        emit::emit_module_body(&mut builder, &mut module, proto, &rt_helpers)?;

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
    pub(super) print_str: FuncId,
    pub(super) print_int: FuncId,
    pub(super) print_bool: FuncId,
    pub(super) print_null: FuncId,
    pub(super) str_concat: FuncId,
}

fn declare_rt_helpers(module: &mut ObjectModule) -> Result<RtHelpers, String> {
    let mut import = |name: &str, params: usize, returns: usize| {
        let mut sig = module.make_signature();
        sig.params
            .extend((0..params).map(|_| AbiParam::new(types::I64)));
        sig.returns
            .extend((0..returns).map(|_| AbiParam::new(types::I64)));
        module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| format!("aot: declare helper {name}: {e}"))
    };
    Ok(RtHelpers {
        print_str: import("varn_rt_print", 2, 0)?,
        print_int: import("varn_rt_print_int", 1, 0)?,
        print_bool: import("varn_rt_print_bool", 1, 0)?,
        print_null: import("varn_rt_print_null", 0, 0)?,
        str_concat: import("varn_rt_str_concat", 5, 0)?,
    })
}
