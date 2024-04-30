use std::{fs::File, sync::Arc};

use cranelift::{
    codegen::{
        control::ControlPlane,
        entity::EntityRef,
        ir::{
            types::{self, I32, I64},
            AbiParam, Function, InstBuilder, MemFlags, Signature, UserExternalName, UserFuncName,
        },
        isa::{self, x64::settings::builder, CallConv},
        settings::{self, Configurable},
        verify_function, Context,
    },
    frontend::{FunctionBuilder, FunctionBuilderContext, Variable},
};

use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

fn main() {
    let mut settings_builder = settings::builder();

    settings_builder.enable("is_pic").unwrap();

    let flags = settings::Flags::new(settings_builder);

    let isa = match isa::lookup(Triple::host()) {
        Ok(a) => a.finish(flags).unwrap(),
        Err(err) => panic!("Error looking up target: {}", err),
    };

    // let call_conv = isa.default_call_conv();

    let object_builder =
        ObjectBuilder::new(isa, "main", cranelift_module::default_libcall_names()).unwrap();

    let mut object_module = ObjectModule::new(object_builder);

    let mut jit_builder = JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();

    let mut jit_module = JITModule::new(jit_builder);

    let mut increment_rutime_sig = object_module.make_signature();

    increment_rutime_sig.returns.push(AbiParam::new(types::I32));
    increment_rutime_sig.params.push(AbiParam::new(types::I32));

    let increment_rutime_id = object_module
        .declare_function("increment_runtime", Linkage::Local, &increment_rutime_sig)
        .expect("this is only a test");

    let jit_increment_rutime_id = jit_module
        .declare_function("increment_runtime", Linkage::Local, &increment_rutime_sig)
        .expect("this is only a test");

    let mut increment_rutime_function = Function::with_name_signature(
        UserFuncName::User(UserExternalName {
            namespace: 0,
            index: 0,
        }),
        increment_rutime_sig,
    );

    let mut func_ctx = FunctionBuilderContext::new();

    let mut increment_builder = FunctionBuilder::new(&mut increment_rutime_function, &mut func_ctx);

    let block = increment_builder.create_block();

    increment_builder.append_block_params_for_function_params(block);
    increment_builder.seal_block(block);

    increment_builder.switch_to_block(block);

    let arg = increment_builder.block_params(block)[0];
    let res = increment_builder.ins().iadd_imm(arg, 1);
    increment_builder.ins().return_(&[res]);

    increment_builder.finalize();

    let mut context = Context::for_function(increment_rutime_function);

    object_module
        .define_function(increment_rutime_id, &mut context)
        .unwrap();

    jit_module
        .define_function(jit_increment_rutime_id, &mut context)
        .unwrap();

    jit_module.finalize_definitions().unwrap();

    let incr = jit_module.get_finalized_function(jit_increment_rutime_id);

    let incr_res = unsafe {
        let code_fn: unsafe extern "sysv64" fn(i32) -> i32 = std::mem::transmute(incr);

        code_fn(1)
    };

    println!("value: {}", incr_res);

    let mut main_sig = object_module.make_signature();

    main_sig.returns.push(AbiParam::new(types::I32));

    let main_function_id = match object_module.declare_function("main", Linkage::Export, &main_sig)
    {
        Ok(a) => a,
        Err(err) => {
            println!("error: {err}");
            panic!("terminate time")
        }
    };

    let mut increment_sig = object_module.make_signature();

    increment_sig.returns.push(AbiParam::new(types::I32));
    increment_sig.params.push(AbiParam::new(types::I32));

    let increment_function_id = object_module
        .declare_function("increment_number_c", Linkage::Import, &increment_sig)
        .unwrap();

    let mut main_function = Function::with_name_signature(
        UserFuncName::User(UserExternalName {
            namespace: 0,
            index: 1,
        }),
        main_sig,
    );

    let increment_ref =
        object_module.declare_func_in_func(increment_function_id, &mut main_function);

    let mut main_builder = FunctionBuilder::new(&mut main_function, &mut func_ctx);

    let block = main_builder.create_block();

    let num = Variable::new(10);

    main_builder.declare_var(num, I32);

    main_builder.seal_block(block);

    main_builder.append_block_params_for_function_params(block);

    main_builder.switch_to_block(block);

    let initial = main_builder.ins().iconst(I32, 10);

    let ret = main_builder.ins().call(increment_ref, &[initial]);
    let ret = main_builder.inst_results(ret)[0];

    main_builder.def_var(num, ret);

    let block2 = main_builder.create_block();

    main_builder.ins().jump(block2, &[]);

    main_builder.seal_block(block2);

    main_builder.switch_to_block(block2);

    let ret = main_builder.use_var(num);

    main_builder.ins().return_(&[ret]);

    main_builder.finalize();
    println!("{}", main_function.display());

    let mut context = Context::for_function(main_function);

    object_module
        .define_function(main_function_id, &mut context)
        .unwrap();

    let res = object_module.finish();

    println!("mangling: {:?}", res.object.mangling());

    let mut file = File::create("output.o").unwrap();
    res.object.write_stream(&mut file).unwrap();
}
