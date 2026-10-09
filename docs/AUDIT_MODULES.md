# AUDIT MÓDULOS VARN — exports, imports, capas, capabilities, host, runtime, ABI, typechecking, resolvers, binders

Fecha: 2026-10-09. Método: solo código + ejecución (Ley 12). Ningún `.md` es evidencia.
Alcance: `crates/varn-binder`, `varn-sem`, `varn-modules`, `varn-resolver`, `varn-checker`, `varn-emit`, `varn-compiler`, `varn-tir`, `varn-types`, `varn-vm`, `varn-builtins`, `varn-contract`, `varn-jit`, `varn-jit-abi`, `varn-pipeline`, `varn-cli`, `varn-rt`.
Principio rector (AGENTS.md §0): secuencia IRs una dirección, fases puras, interfaces portables (nombres+sintaxis, nunca ids internos).

> Breaking permitido (Ley 10): ganancia real verificable en runtime/corrección/determinismo/simplicidad vence compatibilidad. Cada propuesta breaking declara (a) ganancia, (b) verificación, (c) qué se borra. Sin (a)+(b)+(c) no se acepta. Contrapeso: no se autoriza `unsafe`, deuda ni pérdida corrección por micro-ganancia compile-time.

---

## 0. Mapa sistema actual

### 0.1 Archivos clave + structs

Binder / sem:

- `crates/varn-sem/src/resolver.rs:6` `trait ImportResolver { module_bind:7, module_exports:9, stdlib_bind:11, stdlib_exports:13, resolve_specifier:15, record_dep:16, core_exports:19, core_members:25, find_bind_for_type:27, stdlib_specs:46 }`
- `crates/varn-sem/src/bind.rs:68` `struct BindResult { arena:69, scopes:70, ty_table:77 Arc<CheckerTyTable>, interner:75 AtomInterner, annotation_types:80 FxHashMap<AstId,Type>, class_methods:83, type_members:84, class_parents:85, deps:78 Vec<Arc<str>>, source_file:86, extensions:90, core:92 }`. `BindView:249` + `foreign():259`.
- `crates/varn-sem/src/exports.rs:8` `struct ExportMap { symbols:FxHashMap<String,Symbol>, table:Arc<CheckerTyTable> }`. `Deref Target FxHashMap:33`. `assign_slots keys.sort:46-54`.
- `crates/varn-sem/src/symbol.rs:51` `struct Symbol { ty:Option<Type>:53, origin_module:Option<Atom>:66, re_export_path:Vec<Atom>:67, original_name:Option<Atom>:68, alias_node:Option<Box<TypeNode>>:70, slot_idx:75 }`. `SymbolArena:129`.
- `crates/varn-sem/src/scope.rs:18` `CheckerScope { bindings:FxHashMap<Atom,SymbolId>:21, ordered:Vec<SymbolId>:23 }`.
- `crates/varn-sem/src/types/mod.rs:31` `struct Type(pub CheckerTyId, pub Origin)`. `FunctionParam ty:CheckerTyId:60`, `FunctionType return:68`, `ObjectTypeMember ty/key/value:77-91`, `ClassMemberInfo ty:Type:130 + members:131 + symbol_id:137`.
- `crates/varn-sem/src/types/interned/ids.rs:6` `CheckerTyId(u128)`, `:11 TyListId`, `:16 FunctionTypeId`, `:20 ObjectMembersId`. Seeds `0..11:27-38`. `seeded_id:41`.
- `crates/varn-sem/src/types/interned/table.rs:18` `CheckerTyTable { base:Arc<CheckerTyBase>:19, delta_*:FxHashMap:20-23, names:AtomInterner:24 }`. Base `entries/lists/functions/object_members:11-14 FxHashMap`.
- `crates/varn-sem/src/types/interned/hash.rs:6-28` `content_id=xxh3(kind)|1<<127`. `interning.rs:9-26` intern por contenido. `merge.rs:4-80` `absorb if !contains`. `slice.rs:167-215` `slice BTreeMap + absorb_slice if !contains`.
- `crates/varn-core/src/atom.rs:17` `Atom(u128)`, `:20-22 of=xxh3_128`, `intern:71`, `resolve panics:83-85`, `try_resolve:88`, `absorb:92`. `base:Arc<FxHashMap>:29, delta:30`.
- `crates/varn-binder/src/lib.rs:45` `Binder<'r> { resolver:&dyn ImportResolver:46, ty_table:Arc:60, interner:56, class_methods/type_members/class_parents/deps/annotation_types:52-58 }`.
- `crates/varn-binder/src/binder_entry.rs:10` `bind`, `:26 bind_with_global_refs`, `:46 bind_with_globals_iter`, `adopt:92`, `absorb_names:104`.
- `crates/varn-binder/src/imports.rs:7` `bind_import`, `:175 bind_export`, `:209 record_reexport_dep`.
- `crates/varn-binder/src/interface.rs:9` `bind_interface`, `:56 collect_interface_member`.
- `crates/varn-binder/src/core/loader.rs:11` `module_globals`, `:15 merge_core_members`, `:22 build_core_exports`, `:38 build_core_members`.
- `crates/varn-binder/src/context.rs:5` `impl TypeContext for Binder` — `get_interface/class/namespace/enum_members` con `module_bind/stdlib_bind:31-32,46-47,68-69,83-84`.
- `crates/varn-binder/src/binder_intern.rs:10` `adopt = interner.absorb + ty_table.absorb`.
- `crates/varn-binder/src/paths.rs:3` `is_known_module`, `:7 resolve_package_specifier_path`, `:11 resolve_relative`.
- `crates/varn-binder/src/type_infer_member.rs:15-50` `reintern_member_type !contains:22 → stdlib/module_bind:32-34 → absorb:41 → with_origin:42-47`.

Módulos / resolver disco:

- `crates/varn-modules/src/loader.rs:94` `trait ModuleLoader { resolve:95, source:104 }`. `ProviderLoader:107`, `ModuleRegistry:169`, `FilesystemLoader:232`, `MemoryLoader:259`. `default_registry Filesystem+Provider:163-167`. `Registry.source` prueba backends en orden `:216-229`. `FilesystemLoader.source read_to_string:235-255`. `ProviderLoader.source:118-160` solo `Core/Std/Runtime`.
- `crates/varn-modules/src/resolver.rs:18` `ModuleResolver resolve:23` por `Stdlib/Core/Runtime/Relative/Package`.
- `crates/varn-modules/src/artifact.rs:99` `ArtifactKind { ModuleGraph:100, CheckerInterface:102 }`, `write/read_module_artifact:263,280`, `source_fingerprint:236`, `module_key:243`. `ENVELOPE_VERSION=1:95`, `ArtifactClass Cache=1/Distributable=2:128-132`, `write_artifact:306`, `read_artifact:319`, `write_artifact_file WRITE_SEQ+tmp+rename:370`, `prune_superseded GENERATIONS=3:47-89`, `FP OnceLock exe path+len+mtime:15`, `cache_key BUILD^producer:43`, `producer_fingerprint fnv:14-41`, `payload_checksum fnv32:192`, `find_project_root:201`, `global_varn_dir VARN_HOME/HOME:211`, `get_cache_dir VARN_CACHE_DIR else ~/.varn/cache:221`, `get_bytecode_cache_dir:228`, `get_types_cache_dir:232`.
- `crates/varn-modules/src/spec.rs:12` `ModuleSpec`, `provider.rs:7 StdlibProvider + PROVIDER OnceLock:30-32`, `registry.rs:16 core_module_ids, :31 std_module_ids, :35 is_known_stdlib_module`, `layer.rs:2 Layer + check_import:28`, `package.rs:49 resolve_pkg_specifier + resolve_pkg_detailed:65-103 + find_package_root .vn/packages:178-195 + enforce constraint:83 + resolve_export_target exists:117-176 + canonicalize:292-302`.
- `crates/varn-resolver/src/resolver_disk.rs:5` `DiskResolver { loader:ModuleRegistry:6, graph:Mutex<ModuleGraph>:7, in_flight:FxHashSet:9, core_exports/members:11-12 }`. `with_registry:26`, `types_cache_dir:51-54`.
- `crates/varn-resolver/src/graph.rs:9` `ModuleGraph { binds/exports/programs/arenas/resolved_paths/reverse_deps:10-18 FxHashMap }`. `project_root_or_init current_dir+find_project_root:76-80`. `invalidate reverse_deps:83-106`.
- `crates/varn-resolver/src/resolver_trait.rs:9` `impl ImportResolver for DiskResolver` — `module_bind:18`, `module_exports:66`, `stdlib_bind:99`, `stdlib_exports:114`, `resolve_specifier:141`, `core_exports:155`, `core_members:165`.
- `crates/varn-resolver/src/resolver_parse.rs:15` `parse_and_cache scan:16+parse:18`, `:43 bind_and_cache module_globals:52 + Binder::bind:53-56`, `:67 collect`. `is_binding/in_flight:31-42`.
- `crates/varn-resolver/src/resolver_embed.rs:9` `module_exports_uncached`, `:31 load_source Local`, `:58 exports_from_embedded`, `:70 try_load_cache`, `:93 collect base_dir "."`, `:107 bind_from_embedded`. `CarrierKind lib.rs:13-19 File/Bundle/Memory/Native`.
- `crates/varn-resolver/src/exports.rs:58` `collect_exports`, `:48 assign_slots sort`, `:18 rehome_to_declaring_module`, `:264 lookup_global`.
- `crates/varn-resolver/src/cache/cache_io.rs:33` `try_load_cache`, `:91 save_to_cache`, `:65 adopt_dependencies`, `:18 cache_module_id`, `:fingerprint source_fingerprint ^ carrier<<56`.
- `crates/varn-checker/src/checker/mod.rs:38` `Checker<'r> { resolver:39, ty_table:Arc:76 + 12x FxHashMap caches:46-73 }`.
- `crates/varn-checker/src/checker/stages.rs:9` `prepare (module_globals:17 + Binder::bind:22-24 + merge_core_members:28 + enrich:32)`, `:39 init_checker (bind.ty_table.clone:52)`, `:64 run_pass (bind.ty_table=checker.ty_table:81)`, `:88 assemble`.
- `crates/varn-checker/src/checker/type_queries.rs:130` `reintern_foreign_ty absorb+with_origin`, `:120 resolve_bind_atom fallback ty_table.name`, `:152 resolve_type_node_cached`, `:26-44 types_compatible_cached BindView new por llamada key (Type,Type,usize)`.
- `crates/varn-sem/src/codec.rs:8` `serialize_module_interface`, `:20 deserialize_module_interface`. `portable/model.rs:5 PortableModule { exports,bind,names,types }`, `encode.rs:74 from_live`, `decode.rs:8 into_live`.

Runtime / pipeline / host:

- `crates/varn-vm/src/exec/ctx_modules.rs:105` `load_module(spec)` lee `frames.last.source_file`, `:114 load_module_from_source`, `:121 resolve_specifier_from_path`, `:123 linker.cached`, `:127 modules.get FrozenModule thaw`, `:140-155 spec_for pure + build_module nativos no-puros + fallback Std|Core|Runtime`, `:163 is_evaluating → E_BINDING_TDZ`, `:174 precompiled.get`, `:183-191 loader.load JIT fallback + eval_module_proto`, `:197 eval_module_proto reserve_region + export_names sorted assert:204 + alloc_module + set_evaluating:219 + push_frame + run_until + set_done:246`.
- `crates/varn-vm/src/exec/dispatch/modules.rs:7` `op_load_module spec_idx const string`, `:36 op_load_module_slot bounds`, `:79 op_store_module_slot Rc::make_mut:107`.
- `crates/varn-vm/src/exec/jit_helpers/modules.rs:7` `jit_load_module extern C load_module_from_source:28`, `:40 jit_load_module_slot null silencioso`, `:62 jit_store_module_slot panic!`.
- `crates/varn-vm/src/loader.rs:22` `trait ModuleLoader { resolve, load->Option<Rc<FunctionProto>> }`. `:27 CompositeLoader Vec<Box<dyn>>`.
- `crates/varn-vm/src/linker.rs:15` `state:Rc<UnsafeCell<FxHashMap<ModuleId,ModuleLinkState>>>`, `:41 cached`, `:48 is_evaluating`, `:52 set_evaluating`, `:56 set_done`, `:66 share() Rc clone`, `:32-39 table()/table_mut() UnsafeCell + mut_from_ref`.
- `crates/varn-vm/src/vm.rs:22` `Vm::new(precompiled:Rc<FxHashMap<ModuleId,Rc<Proto>>>)`, `:30 with_loader(Arc<dyn>)`, `:137 prefill_native_modules build_module+convert_to_module_obj+insert`, `:164 freeze_pure_modules scratch Vm::new:175 + loader:177 + freeze+set_done + loop pure_ids scratch.ctx.load_module:203`.
- `crates/varn-vm/src/exec/ctx.rs:36` `precompiled`, `37 loader Option<Arc<dyn>>`, `67 capabilities:Rc<CapabilitySet>`, `123 default allow_all()`, `266 fork_for_task clona globals,heap,modules,precompiled,loader,capabilities,linker.share()`, `32 globals:Rc<UnsafeCell<GlobalStore>>`, `35 modules:Rc<UnsafeCell>`, `48 proto_constants`, `49 static_closures`, `68 metadata`, `70 hashable_keys Rc<UnsafeCell<FxHashMap>>`, `251 globals_ref/mut UnsafeCell`, `108 gc_roots itera globals.values+modules+static_closures+proto_constants+metadata`.
- `crates/varn-vm/src/exec/host/isolates.rs:230` `spawn_isolate thread::spawn + Vm::new(empty):257 + with_loader:263 + load std:task:266 + load path:274` sin caps clonadas, sin precompiled heredado.
- `crates/varn-builtins/src/modules/runtime/task/task.rs:40` `ctx.load_module(resolved_path)` host también carga runtime.
- `crates/varn-vm/src/exec/modules.rs:4` `resolve_specifier_to_id ModuleResolver::new()` fresco cada llamada.
- `crates/varn-vm/src/globals/mod.rs:8` `GlobalStore { values:Vec<VmValue>, names:FxHashMap<Arc<str>,usize>, idx_to_name }`, `:23 with_native_layout NATIVE_GLOBALS order`, `:33 panic! sin valor`, `:56 reserve_region push null`, `:66 define overwrite`, `:88 set_by_index resize`, `:106 get_by_index_unchecked get_unchecked`, `:116 set_by_index_unchecked get_unchecked_mut`.
- `crates/varn-emit/src/module_globals.rs:63` `assign_global_slots`, `72 FxHashMap<Arc<str>,u32>`, `103 Dynamic(NotYetSupported) fallback`, `121 global_names`.
- `crates/varn-emit/src/lib.rs:32` `emit_module(program,arena,bind,expr_table,call_mappings,desugar)->TirModule`, `169 imports=prelude+collect_imports`, `173 source_file`.
- `crates/varn-emit/src/imports_exports.rs:7` `collect_exports`, `78 collect_imports`, `124 math_intrinsic_imports std:math wire`.
- `crates/varn-tir/src/module.rs:117` `TirModule { source_file,imports,exports,types,classes,enums,signatures,functions,globals,global_names,class_defs,top_level }` no `Serialize`.
- `crates/varn-pipeline/src/execute.rs:64` `boot_machine(precompiled,caps)`, `69 PipelineLoader::new`, `71 Vm::new().with_loader`, `72 ctx.capabilities=Rc::new(caps)`, `26 execute() default allow_all()`.
- `crates/varn-pipeline/src/opts.rs:25` `RunOpts::default capabilities=allow_all`.
- `crates/varn-pipeline/src/stdlib_loader.rs:248` `serialize_module_interface`, `258 postcard bytecode`, `261 BundleModule { id,pure,interface,bytecode,source }`, `273 StdBundle { std_version, host_api_version:HOST_API_VERSION }`, `:17 ProtoBytesCache FxHashMap`, `:28 cached_proto postcard Proto`, `:81 source_fingerprint+module_key`, `:72 source.bytecode postcard directo`, `:11 PipelineLoader { registry:default_registry:22, protos, session:Session::new():24 }`.
- `crates/varn-pipeline/src/cache.rs:10` `compile_cache_path get_bytecode_cache_dir`, `29 load_cached_graph valida source_hashes entry+deps vía fs::read o provider.embedded_source/source_path`, `115 store_cached_graph`, `154 compile_output_from_graph ModuleId::from_canonical_str(path)`.
- `crates/varn-pipeline/src/portable.rs:8` `write_portable Distributable`, `28 from_bytes`.
- `crates/varn-pipeline/src/module_precompile.rs:104-118` `in_degree:FxHashMap + topo_queue desde in_degree.iter().filter`, `:185-225 sorted.rev() por módulo Checker::check_with + report_diagnostics + emit_and_compile + modules.insert`, `:182-184 entrada excluida loop insertada antes`, `:35,75 source_hashes fnv1a64`, `:258 resolve_import_specifier duplica Stdlib/Core/Runtime/Relative/Package+package_nodes`, `:302-315 PackageNode integrity fnv1a64`.
- `crates/varn-pipeline/src/compile.rs:22-41` `emit_and_compile: emit_module + from_tir::compile_module`, `13-20 sorted_export_names names.sort()`, `119-121 graph_hash=source_hashes.values().fold(wrapping_mul/add)`.
- `crates/varn-pipeline/src/resolver.rs:10` `Session::new DiskResolver::with_registry(default_registry)`.
- `crates/varn-pipeline/src/hash.rs:1-8` `fnv1a64`.
- `crates/varn-types/src/module_graph.rs:16-23` `ModuleGraphArtifact{entry_path,graph_hash,source_hashes,modules,package_nodes}`.
- `crates/varn-core/src/module_id.rs:47-77` `ModuleId::stdlib|local_str|from_canonical_str`.
- `crates/varn-core/src/opcode.rs:135` solo `LoadModule`, `137 LoadModuleSlot`, sin `ImportDynamic`.

Capabilities / host / ABI:

- `crates/varn-types/src/capabilities.rs:4` `CAP_FS_READ=1<<0`, `5 WRITE`, `6 NET_CLIENT/SERVER`, `8 ENV/EXEC/FFI`, `11 CAP_ALL=MAX`, `:14 CapabilitySet { mask, fs_read_paths, fs_write_paths, net_hosts, net_ports, env_vars }`, `:28 Default=allow_all()`, `:35 allow_all`, `:46 sandbox mask 0`, `:58 has_mask`, `:62 has_capability _ => true`, `:75 check_fs_read`, `:85 check_fs_write`, `:95 check_net_connect ends_with`, `:107 check_net_listen`, `:117 check_env`, `:130 check_ffi/exec`, `:140 path_matches_any canonicalize.or(fallback)`.
- `crates/varn-types/src/native_ctx.rs:305` `load_module default Err(not supported)`, `176 capabilities() static DEFAULT_CAPS=allow_all`, único wiring real `crates/varn-vm/src/exec/host/mod.rs:397 capabilities()->&self.capabilities`, `367-369 ExecCtx::load_module` usado por nativos/isolates sin validación caps.
- `crates/varn-types/src/native.rs:5` `NativeFn=fn(&mut dyn NativeCtx,&[VmValue])->NativeFnResult`, `:83 NativeOpEntry { module_id,namespace,symbol,func_ptr,raw_func_ptr,signature,capability_mask,entry_kind } #[repr(C,align16)]`, `:104 Sync/Send`, `:130 func() transmute`.
- `crates/varn-cli/src/commands/run.rs:39` `build_capabilities: sandbox:41, allow_all||!has_any_allow=>allow_all:50, mask CAP_FS_READ:61 etc, env_vars:108 pero net_ports:None:107 nunca seteado`.
- `crates/varn-cli/src/commands/check.rs:14,debug.rs:37,eval.rs:14,repl.rs:67,tester.rs:123` usan `Default=all`. `crates/varn-dap/src/driver.rs:52` `boot_machine(...,allow_all())`.
- `crates/varn-builtins/src/dispatch/mod.rs:128` `capability:None` hardcodeado `build_table`, `196 dispatch_runtime_op` check muerto, `180 describe_op capability:None`, `:10 unsafe extern C __VARN_OPS_START/END`, `:29 START_MARKER`, `:33 END_MARKER`, `:36 iter_native_ops from_raw_parts`, `:62 FALLBACK_ENTRIES OnceLock<Mutex>`, `:72 ALL_OPS OnceLock`, `:110 ENTRY_BY_OP_ID`, `:120 TABLE`, `:140 MODULE_OPS`. `entry.rs:9 capability:Option<&'static str>` existe pero sin uso.
- `crates/varn-builtins/src/modules/runtime/ffi/ffi.rs:21` `!has_capability("sys.ffi") x12 (21,39,54,72,165,186,204,224,245,263,282,304)`, `13 ERR_PERMISSION_DENIED`.
- `crates/varn-builtins/src/modules/runtime/fs/fs.rs:46` `check_fs_read/write (121,129,163,170,193,200,207,221,233,245,252,259,266,273,287)`.
- `crates/varn-builtins/src/modules/runtime/net/net.rs:34` `check_net_listen`, `49 check_net_connect`, `142`.
- `crates/varn-builtins/src/modules/runtime/sys/sys.rs:29` `check_env`, `35`.
- `crates/varn-contract/src/varn_contract/member.rs:182` `wrap_ident`, `193 fast_path`, `210 NativeOpEntry func_ptr=wrap as *const u8`, `218 raw_func_ptr`, `219 signature`, `139 capability_mask:0`, `220,269 capability_mask:0`.
- `crates/varn-contract/src/varn_contract/fast.rs:72` `pub extern C fn fast_wrap<__T>(this:VmValue|i64|f64|bool...)`, `55 DummyCtx decode, fallback Default silencioso`.
- `crates/varn-contract/src/varn_contract/mapping.rs:53` `mapped_tag_path`, `77 param_ty`, `96 owned_ty`, `113 ret_ty`, `150 map_to_arg_type`, `171 is_fast_eligible !fallible+Function|StaticMethod+scalar`, `189 signature_token param_count u8, 7 slots`.
- `crates/varn-vm/src/exec/ctx_gc.rs:20` `invoke_native enter_native/timed_native/exit_native`.
- `crates/varn-vm/src/exec/jit_helpers/natives.rs:6` `jit_call_native_window extern C`, `30 transmute<usize,NativeFn>(fn_addr) si !=0 else native_op_fn(op_id)`.
- `crates/varn-jit-abi/src/lib.rs:115` `JitHelpers { ..., load_module:171, load_module_slot:172, store_module_slot:202, jit_call_native_window:163, resolve_native_op:227, ... offsets }`, `:3-243 JitArrayLayout/JitCallLayout/JitHelpers #[repr(C)]`.
- `crates/varn-rt/src/lib.rs:3` `unsafe extern C _varn_main`, `10 main`, `16 varn_rt_print(ptr,len)`, `26 print_int`, `31 print_bool`, `46 str_concat->StrResult Box::leak`, `65 panic->exit(1)`.
- `crates/varn-jit/src/clif/native_abi.rs:6-101` `NativeClass{Word|Float|Boxed}`, `NativeShape::of_proto`, `signature()`, `NATIVE_FRAMELESS=1<<63`, `id(): ret(2b)+len+2b/clase`, `MAX_ID_PARAMS=27`.
- `crates/varn-jit/src/clif/lower.rs:22-35` `native_entry: has_rest|!Native→(0,0) sino (raw,id|frameless)`.
- `crates/varn-jit/src/clif/from_ssa/call/native_entry.rs:78-95` `carga proto_native/proto_native_sig/proto_epoch, same_sig && same_epoch && has_entry sino slow path; baked_epoch=linker.current_epoch()`.
- `crates/varn-jit/src/clif/from_ssa/extra/modules.rs:47-94` `load_module/load_module_slot/store_module_slot vía h.load_module...`.
- `crates/varn-builtins/src/modules/core/globals/globals.rs:5` `SILENT:AtomicBool`, `7 set_print_silent`.
- `crates/varn-builtins/src/runtime/timer.rs:39` `WHEEL:OnceLock<Mutex>`, `115 PURGED:AtomicU64`, `channel.rs:36 REGISTRY:OnceLock`, `37 NEXT_ID:AtomicU64`, `sqlite.rs:7 DB_POOL:Mutex<Vec>`.
- `crates/varn-vm/src/profile.rs:63` `TASK_STATS:static`, `alloc_profile.rs:32 LEVEL:AtomicU8`, `34 thread_local CYCLES/HITS`, `clif_link.rs:11 thread_local CURRENT_CTX/CURRENT_EPOCH/COMPILED`, `29 NEXT_EPOCH:AtomicU64`, `scheduler/pumps.rs:23 thread_local PUMPS/HOSTS RefCell`.

Typechecking cross-module / ABI serializada:

- `crates/varn-sem/src/bind.rs:259-267` `BindView::foreign(origin): si origin==source_file → None sino module_bind||stdlib_bind`. `:282-313 getters` devuelven clon foráneo. `:348-361 resolve_type_alias` delega foráneo recursivo.
- `crates/varn-sem/src/compat/resolve.rs:8-15` `resolve_atom: interner.try_resolve || ty_table.name`. `nominal.rs:24-40 Named|Generic compara resolve_atom(dn),resolve_atom(in_) + orígenes, fallback _=>dn==in_ (Atom crudo si irresoluble)`. `:178-213 named_fallback: resolve_type_alias(name_s,origin_s) + re-llama types_compatible_impl`.
- `crates/varn-checker/src/checker_expressions/members/owner.rs:17-29` `foreign_owner: origin.filter(!=source_file) → module_bind||stdlib_bind + declares_type`. `member_named.rs:75-79 si foreign_owner find_member_info_uncached + reintern_foreign_ty`. `:188-244 find_bind_for_type → search_named_in: clases/interfaces/enums/métodos foráneos cada ty reintern`. `member_named.rs:45-67 Namespace *`, `member_type.rs:146-176`, `member_exists.rs:147-167 is_known_module→stdlib_exports sino module_exports exports.get(key)`, `member_exists.rs:49 find_bind_for_type`, `foreign_classes.rs:9-28,41-73 collect_foreign_inherited_fields filtra class_parents origin.is_some() ordena BTreeMap ancestor_fields owner=module_bind||stdlib_bind reintern_foreign_ty(owner,m.ty)`, `foreign_enums.rs:41-61 solo Named|Generic origin!=source_file → module_bind||stdlib_bind → enum_layout`.
- `crates/varn-checker/src/checker/stages.rs:80-85` post-pass `collect_foreign_inherited_fields + bind.ty_table=checker.ty_table + bind.interner.absorb(checker.ty_table.names()) + collect_foreign_enums`.
- `crates/varn-sem/src/types/type_impl.rs:121-123` `sanitize_foreign(self)->Type{self}` no-op. Fallbacks locales `Dynamic`: `member_named.rs:55 sym.ty.unwrap_or(Dynamic)`, `member_type.rs:158`, `recorder.rs:114`, `output.rs:99-110 resolve_at unwrap_or(Dynamic)`, `type_impl.rs:125-148 get_array_element_type no-array→Dynamic; 24-36 apparent literales mezclados→Dynamic`.
- Degradación `emit::lower→BackendTy::Dynamic`: `varn-emit/src/ty/lower.rs:19-21 opaque()=Dynamic(NotYetSupported)`, `:23-29 resolve_named sin class_id|enum_id→opaque`, `:31-48 foráneo no-is_local_origin solo foreign_enum_id sino opaque; clases foráneas→opaque`, `:107-114 Named|Generic|EnumVariant desconocido→opaque`, `:118-144 Object→Dynamic(IndexSignature), Fn|Intersection|This|Template|Typeof|KeyOf|Indexed|Mapped|Conditional|Infer|Predicate→opaque`, `body/scope.rs:286-294 expr_ty sin entrada→Dynamic`, `body/expr_member.rs:254-266 campo sin ClassInfo::field→ByName(NotYetSupported)`.
- Segunda degradación `BackendTy→HirType→SlotKind`: `varn-compiler/src/from_tir/ty.rs:4-43 Decimal|BigInt|Void|Never|Dynamic→Dynamic; Char|Bytes|Tuple|Enum|Fn→Ref; Class sin tir.class(cid)→Ref`, `:46-51 resolve !contains(id)→Dynamic`, `ssa/emit/register_meta.rs:42-59 Nullable|Dynamic→Dynamic; resto heap→Ref`, `:27-32 conflicto SlotKind mismo registro→Dynamic`.
- Compilación separada cada módulo `CheckResult→TIR→Proto`: `module_precompile.rs:185-225 sorted.rev() por módulo Checker::check_with + report_diagnostics + emit_and_compile + modules.insert(path,proto)`, `compile.rs:22-41 emit_and_compile: emit_module + from_tir::compile_module(tir,export_names,source)`, `emit/src/lib.rs:32-49 emit_module crea TyTable fresca por módulo + prime(NEVER=0) + tables::build(bind,desugar)`, `compiler/src/from_tir/compile.rs:108-151 compile_module verifica TIR compila solo top_level export_names→proto.export_names`, `from_tir/build/exports.rs:42-75 build_imports: por tir.imports emite LoadModule{source}+GetProperty Dynamic+global_store; is_type_only→skip. 7-40 build_exports vía LoadModule/GetProperty/StoreModuleSlot`.
- Hash identidad: `pipeline/hash.rs:1-8 fnv1a64`, `module_precompile.rs:35,75 source_hashes fnv1a64(source)`, `types/src/module_graph.rs:16-23 ModuleGraphArtifact{entry_path,graph_hash,source_hashes,modules,package_nodes}`, `modules/artifact.rs:236-248 source_fingerprint DefaultHasher + module_key hash(ModuleId)+fingerprint`, `core/module_id.rs:47-77 stdlib|local_str|from_canonical_str`, `tir/src/ty.rs:1-23 TyId/ClassId/EnumId/SigId/FnId/ModuleId/LocalId(u32)`, `verify/handles.rs:29-33 Class(c) if class(c).is_none→bad`.
- Caché bytecode grafo entero: `pipeline/cache.rs:10-27 compile_cache_path stem.hash(path_canonical).cache_key.vncache`, `:29-113 load_cached_graph: read_artifact(ModuleGraph) verifica entry_hash + cada source_hashes[path] contra disco/provider.embedded_source|source_path Ok(None) ante cambio`, `:115-152 store_cached_graph postcard→write_artifact(ModuleGraph,Cache)`, `:154-182 compile_output_from_graph precompiled ModuleId::from_canonical_str(path)→proto salta entrada`.
- Caché interfaces checker por módulo: `resolver/cache/cache_io.rs:18-31 cache_module_id(stdlib? vs local) + cache_fingerprint=source_fingerprint ^ carrier<<56`, `:33-63 try_load_cache: read_module_artifact(CheckerInterface) + deserialize_module_interface verifica bind.source_file==virtual_id||canonical + adopt_dependencies + exports.table=bind.ty_table`, `:91-113 save_to_cache serialize`, `sem/codec.rs:8-26 serialize absorb(exports.table+bind.ty_table)+PortableModule::from_live→postcard; deserialize into_live(new Interner,new Table)+assign_slots`, `portable/encode.rs:75-101 from_live roots exports+bind + table.slice(roots) + names solo átomos alcanzables`, `portable/decode.rs:8-25 into_live intern(names)+absorb_slice + bind.interner/table reemplazados`, `graph.rs:83-106 invalidate(ModuleId) vía reverse_deps`.
- Versionado artefacto único existente: `modules/artifact.rs:12 include!(OUT_DIR/build_fingerprint.rs)→BUILD_FINGERPRINT`, `:43-45 cache_key()=BUILD_FINGERPRINT ^ producer_fingerprint()`, `:14-41 producer_fingerprint fnv(exe_path+len+mtime)`, `:95-149 ENVELOPE_VERSION=1 + ArtifactKind{ModuleGraph=1,CheckerInterface=2,StdBundle=3} + ArtifactClass{Cache=1(+producer_stamp),Distributable=2(=0)}`, `:306-368 write_artifact MAGIC+VARN+ENVELOPE+kind+class+BUILD_FINGERPRINT+producer+checksum; read_artifact → Superseded si schema!=BUILD_FINGERPRINT o producer!=stamp, Corrupt si checksum falla`.
- ABI nativa/jit sin versión, guardas por `sig+epoch` runtime: `tir/src/ty.rs:39-61 BackendTy + field_kind:64-91 + TyTable:133-175`, `types/src/register_meta.rs:4-46 SlotKind Int|Float|Bool|Str|Ref|Dynamic; Bool|Str→Dyn + FrameLayout::for_proto:60-77`, `types/src/chunk/proto/definition.rs:36-45 FunctionProto register_meta,param_kinds,return_kind con serde(default) + jit_native_sig/jit_epoch/jit_entry...:#[serde(skip)] 50-113 firma nativa no persiste se recalcula`.

Cómo resuelven imports (binder):

- `varn-binder/src/imports.rs:8-32`: `check_import(Layer::of_module):10`, `is_relative ./ /:18`, `is_package is_pkg_specifier:19`, `base_path parent(source_file):22-23`, `resolve_package_specifier_path:26` vs `resolver.resolve_specifier:28`. `resolved_target None => InvalidImportPath:59-67`.
- `:36-50`: `resolver.module_exports(abs, visiting):38` relativo; `resolver.stdlib_exports:44` stdlib; `adopt(&exports.table):49` absorb tabla ajena en propia. `deps.push:51-56`.
- `:78-172`: `Named/Default/Namespace:80-97`, `module_path_atom intern_local:109`, `exports.get(imported):125` clone + `original_name:133` + `origin_module.or(module_path_atom):134` + `ty.with_origin:135-140` + `intrinsic_wire lookup mp/imported:142-145`. Miss → `UnknownSymbol + Let fallback:150-164`. `*` → `Namespace + Named("*"):115-123`.
- Reexport solo registra dep `:195-222`: `ExportDecl::Named source / All:201-205` → `record_reexport_dep (is_known_module:211 else resolve_specifier:217, deps.push:220)`. Sin resolución símbolos ahí.
- Contexto resuelve tipos foráneos bajo demanda `context.rs:27-38,41-57,63-91`: si `origin != source_file`, `module_bind(origin).or(stdlib_bind)` y lee `rb.type_members.*` clonado. Igual `sem/bind.rs:259-267 BindView::foreign`.
- Disco `resolver_trait.rs:141-149 resolve_specifier` → `varn_modules::resolver::resolve_specifier_path + store_path` caché. `module_bind:18-64`: `cached_bind:19`, `canonical_or_original:23`, `load_source Local:31`, `try_load_cache:36`, `parse_and_cache:48`, `bind_and_cache:50`, `collect:53-60`, `save_to_cache:61`. `module_exports:66-90`: `cached_exports:67`, ciclo `visiting contains canonical → empty:78-80`, placeholder `store empty:82` anti-recursión, `module_exports_uncached:85`, `store:88`. `stdlib_bind:99-112 + stdlib_exports:114-139` vía `ModuleId::stdlib + load_source Provider + bind/exports_from_embedded`. `core_exports:155-163` y `core_members:165-172` memoizados `Mutex<Option>` vía `varn_binder::core::loader::build_*`.
- `resolver_parse.rs:52-57 bind_and_cache` elige `module_globals(key)` → `bind_with_global_refs` else `bind`. `is_binding/in_flight:31-42` evita reentrada.
- `resolver_embed.rs:31 load_source Local`, `:70 try_load_cache`, `:93 collect base_dir "."` para stdlib. Distingue `File/Bundle/Memory/Native` vía `CarrierKind:lib.rs:13-19`.
- Módulos bajo `modules/resolver.rs:23-61`: `Stdlib→Std, Core→Core, Runtime→Runtime, Relative join+normalize:30-45, Package resolve_pkg_specifier:56-58`. `package.rs:65-103 resolve_pkg_detailed: split:69, find_package_root ancestors .vn/packages:178-195, load manifest:78, enforce constraint:83, resolve_export_target exists checks:117-176, canonicalize:292-302`. `loader.rs:216-229 Registry.source` prueba backends en orden, `FilesystemLoader.source:235-255` solo `Local`, `ProviderLoader.source:118-160` solo `Core/Std/Runtime`.
- Checker usa binder + resolver directo `stages.rs:16-33`, `foreign_classes.rs:41-45 module_bind.or(stdlib_bind)`, `member_exists.rs:49 find_bind_for_type`, `exhaustiveness.rs:104 origin_atom → module_bind/stdlib_bind`, `type_queries.rs:161 BindView + resolve_type_node`.

Cómo exportan símbolos:

- Colector central `resolver/exports.rs:58-241` itera `Stmts Decl::Export`. Ramas: `Decl:77-106 decl_primary_name:243 (solo primer declarator Identifier; resto None) + lookup_global bind:80 clone + origin_module=intern_origin(abs_path):82 + out.insert. SumType variants:86-95, Enum members:96-105 expandidos individual. Named sin source:107-126 lookup_global local:115 + rehome_to_declaring_module:116 (si origin != bind.source_file recarga module/stdlib_exports y adopt_exports absorb:41-45) + name=exported:118 + origin.or(abs_path):119-122. Named con source:127-152 resuelve is_known_module? stdlib_exports:133 else resolve_relative + record_dep + module_exports:136-138, adopt_exports:140, luego src_exports.get(local):144 + rehome:145 + re_export_path.push abs_path:148. All sin alias:153-177 mismo resolve + adopt + copia todo src_exports.iter() salvo colisión contains_key skip:169. All con alias ns:178-213 crea Namespace ns_sym:198 con Named("*",origin src_abs):201-204 + origin_module src_abs:205 además aplana ns.sub:210 + ns_sym:212. Default:214-238 Function/Class lookup_global + name=default:219-229, Expr → Let default:233-237`.
- Slots deterministas `exports.rs:48-56` + `sem/exports.rs:46-54`: `keys.sort()` luego `slot_idx`.
- Binder `bind_export:imports.rs:175-207`: `Decl → bind_decl:178` (símbolo queda global, colector posterior lo recoge); `Default Fn/Class → bind_function/bind_class`; `Named/All` solo `escape_array + record_reexport_dep`, sin definir símbolos.
- `ExportMap.table = bind.ty_table.clone()` en `resolver_parse.rs:76`. Origen internado en tabla exports `exports.rs:10-12 intern_origin`, no en binder interner solo.
- Core agregación `binder/core/loader.rs:22-36 build_core_exports`: loop `core_module_ids` + `stdlib_exports` + `table.absorb:27` + `origin_module.or(origin):31` + `symbols.insert:32`. `build_core_members:38-83` absorbe `ty_table:42` y clona `class/interface/enum/namespace/flattened/methods/parents` marcando `is_builtin_or_intrinsic=true:59`.

Dónde cruzan ids internos:

- `Type(CheckerTyId)` viaja en `Symbol.ty symbol.rs:53`, `ClassMemberInfo.ty types/mod.rs:130`, `FunctionParam.ty:60`, `ObjectTypeMember:77-91`, `annotation_types/match/evolved bind.rs:80-97`. Todo clonado entre módulos sin re-internar salvo `with_origin/reintern`.
- `Atom` viaja en `Symbol.name/type_params/doc/origin_module/re_export/original symbol.rs:52-68`, `TypeKind::Named(Atom,Option<Atom>) / Generic(Atom,TyListId,Option<Atom>) ids.rs:23`. `Atom=xxh3_128 texto atom.rs:20-22` — mismo texto mismo id global, pero `resolve` exige presencia en interner local (`resolve panics:83-85`, `try_resolve:88`). Cruce frágil si tabla no absorbida.
- Puntos cruce: `binder/imports.rs:114-140 exports.get clonado otro módulo ty.with_origin(origin,&mut ty_table local):136` — crea nuevo `CheckerTyId` local pero resto `Symbol` (constraints,members) conserva ids viejos. `binder/binder_intern.rs:10-12 adopt: interner.absorb + ty_table.absorb` — única sutura; llamada en `bind_import:48-50`, `binder_entry:92 globals_table`, `type_infer_member:41 absorb`. `type_infer_member.rs:15-50 reintern_member_type: si !table.contains(ty.0):22 stdlib/module_bind(origin):32-34 table.absorb(b.ty_table):41 with_origin si Named/Generic sin origen:42-47`. Sin absorb → `get` panic otra tabla. `binder/context.rs:30-34,44-49,66-72,81-86` devuelve `Vec<ClassMemberInfo>` foráneo con `Type` ids foráneos sin absorb; consumidor debe `reintern`. `sem/bind.rs:282-313 BindView getters` igual devuelve clon foráneo. `resolver/exports.rs:14-16 adopt_exports absorb`, `:25-45 rehome` adopta tabla mapa origen en `out.table`. `cache_io.rs:58 exports.table = bind.ty_table.clone` unifica tras `deserialize`. `checker/type_queries.rs:130-143 reintern_foreign_ty: if !contains absorb:131-133 luego with_origin si sin origen:134-142`. `foreign_classes.rs:70-73` lo aplica a `m.ty` heredado. `member_atom.rs:3-6 resolve_atom_text bind.interner.try_resolve or bind.ty_table.name` — doble fuente. `sem/portable/model.rs:5-10` serializa `ExportMap + BindResult` con ids crudos + `names:Vec<Box<str>> + types:TySlice`. `encode.rs:15-72 Roots` recoge `CheckerTyId` de symbols/members/methods/fields + `Atom` de names/origins; `decode.rs:14-18 intern(name)+tables.intern_name + absorb_slice`, luego `bind.interner=interner.clone:20`, `bind.ty_table=table:21`. `codec.rs:12-16` fusiona `names absorb + table absorb` antes `from_live`; `postcard to_allocvec:16`. `checker/stages.rs:81-82 bind.ty_table=checker.ty_table.clone + bind.interner.absorb(checker.ty_table.names())` — checker muta tabla binder tras chequeo. `loader.rs:17-18 merge_core_members absorb core.table en bind.ty_table`, `cache_io.rs:84-85 adopt_dependencies absorb dep ty_table + interner.absorb names` en carga caché. Panic si id ajeno sin absorb `resolution.rs:6-13 get panic CheckerTyId`, `:19-26 TyListId`, `:28-34 FunctionTypeId`, `:36-43 ObjectMembersId`.

Ciclos dependencias:

- Cargos: `varn-binder/Cargo.toml:7-9` depende `varn-sem,varn-core,varn-modules`. `varn-resolver/Cargo.toml:7-12` depende `varn-sem,varn-binder,varn-modules,varn-lexer,varn-parser`. `varn-checker/Cargo.toml:7-11` depende `varn-core,varn-sem,varn-binder`. `varn-sem/Cargo.toml:7-11` solo `varn-core` + serde/postcard. Sin ciclo Cargo, pero ciclo runtime vía `dyn ImportResolver`: `binder_entry.rs:14,30,50 resolver:&dyn ImportResolver` + `imports.rs:38 module_exports` + `context.rs:31 module_bind` → binder llama resolver. `resolver_parse.rs:52-56 Binder::bind` + `resolver_trait.rs:160-161 build_core_exports(self)` + `:169 build_core_members(self)` + `cache_io.rs:78-81 stdlib/module_bind` → resolver llama binder. `checker/stages.rs:22-24 Binder::bind` + `checker/type_queries.rs:162-166 varn_binder::resolve_type_node` → checker llama binder; binder llama `varn_sem::compat::types_compatible: type_resolution/keyed_access.rs:214`, checker llama mismo compat con caché propia `type_queries.rs:37-44`. `binder/paths.rs:4 is_known_module → varn_modules::resolver`, `imports.rs:19 is_pkg_specifier`, `context` + `resolver` usan `ModuleId::local_str/stdlib: resolver_disk.rs:36,100,115`.
- `DiskResolver { loader:ModuleRegistry, graph } resolver_disk.rs:6-7` rompe solo a medias: loader alto inyectado vía `with_registry:26`, pero `default_registry Filesystem+Provider: loader.rs:163-167` ata disco por defecto.

HashMap no determinista:

- `FxHashMap/FxHashSet` en rutas que deciden orden/export/intern: `binder/lib.rs:52-73` todos `class_methods,class_parents,annotation_types,match_arm,sum_*,evolved,type_decls:FxHashMap`; `reported_*:FxHashSet:67-68`. `sem/bind.rs:43-59 TypeMembers/Extensions FxHashMap`, `:83-99 BindResult idem`, `scope.rs:21 bindings FxHashMap<Atom,SymbolId>`, `cores.rs:9-23 CoreMembers/Exports FxHashMap`, `exports.rs:9 symbols FxHashMap`, `types/interned/table.rs:11-24 entries/lists/functions/members FxHashMap`, `atom.rs:29-30 base/delta FxHashMap`. `resolver/graph.rs:10-18 binds/exports/programs/arenas/resolved_paths/reverse_deps FxHashMap`, `in_flight FxHashSet resolver_disk.rs:9`. `checker/mod.rs:46-73` 10x `FxHashMap` caches (`narrowed,infer,compat,member_exists/type,map_generics`) + `FxHashSet active/abstract/warned`.
- Iteraciones sin orden: `portable/encode.rs:35-53 tm.classes.values/interfaces.values/objects.values/enums/namespaces/flattened + getters/setters.values` sin sort — orden absorción/slice varía; `core/loader.rs:24,40-79 for spec in core_module_ids (orden provider) + for (k,v) in rb.type_members.* + for (name,sid) in scope.bindings:44` itera `FxHashMap`; `resolver/exports.rs:168 src_exports.iter(), :206 iter()` orden hash al copiar `All`; `package.rs:124 for (key,val) in manifest.exports FxHashMap:124` wildcard match orden hash; `type_impl.rs:241 seen FxHashSet` en `union` + `table.rs:86-93 freeze extend drain` orden hash.
- Determinista parcial: `assign_slots keys.sort exports.rs:49-50`, `foreign_classes classes.sort foreign_classes.rs:19`, `foreign_enums/foreign_classes BTreeMap foreign_classes.rs:2,12,20`, `encode Roots atoms BTreeSet encode.rs:5`, `artifact module_key/source_fingerprint DefaultHasher artifact.rs:236-248` — `DefaultHasher` SipHash claves aleatorias → fingerprint/cache key no estable entre procesos.
- `scope.ordered Vec scope.rs:23` + `global_symbols ordered.map bind.rs:144-147` sí determinista; `bindings HashMap` no.

Disco/IO en fases bajas:

- `modules/loader.rs:240 FilesystemLoader.source read_to_string`, `:144 ProviderLoader source_path read_to_string`, `:240-249 Io/NotFound`, `package.rs:252 read_to_string manifest`, `:296 canonicalize`, `:372 canonical_or_original canonicalize`, `:378-380 resolve_specifier_path join+exists`, `artifact.rs:271 create_dir_all`, `:287 read`, `:375 write`, `:376 rename`, `:65 read_dir`, `:85 remove_file`, `:25-34 current_exe+metadata+modified`, `:202-209 find_project_root exists`, `:211-226 global_varn_dir env VARN_HOME/HOME + VARN_CACHE_DIR:222`.
- `resolver/graph.rs:76-80 project_root_or_init current_dir + find_project_root`, `cache_io.rs:44 read_module_artifact`, `:105 write_module_artifact`, `resolver_trait.rs:31 load_source`, `:52 parse_and_cache`, `resolver_disk.rs:51-54 types_cache_dir`.
- Binder fase baja toca IO vía trait + paths: `imports.rs:22 parent(source_file) + resolve_specifier:28 / resolve_package:26 + module_exports:38`, `record_reexport_dep:213-217 parent+resolve`, `paths.rs:24-26 resolve_specifier`, `core/loader.rs:25 stdlib_exports, :41 stdlib_bind` en `build_*` llamado desde `checker/stages.rs:17,28` y `resolver_trait core_*`. Checker `prepare stages.rs:16-33` dispara IO transitiva antes de chequear.

Capas:

- `varn-modules/src/layer.rs:1-71`: `Layer { Core, Runtime, Std, User }`, `of_module por prefijo CORE_PREFIX/RUNTIME_PREFIX/STD_PREFIX o in_source_tree`, `check_import(from,specifier)`: `Relative siempre Ok:29-34`, matriz `(Core,Core)|(Runtime,Std)|(Std,Std|User)|(User,User) Ok`, resto Err con mensaje. Test `layer_matrix:60-70`.
- Solo llama `varn-binder/src/imports.rs:10`. Ni `ctx_modules.rs`, ni `module_precompile.rs:258`, ni `DiskResolver`, ni `PipelineLoader` verifican. `of_module` por string + `in_source_tree` — spoof posible `std:../user` si no normaliza antes.

---

## 1. Hallazgos P0 (subsanar ya, breaking permitido)

### H1. Capabilities fail-open por defecto

Evidencia:

- `varn-types/src/capabilities.rs:28-31 Default=allow_all()`, `:62-72 has_capability _=>true`, `:95-104 ends_with`, `:140-144 canonicalize.or(fallback)`, `:75-93 check_fs_* None=>true`, `:99-113 check_net_* None=>true`.
- `varn-types/src/native_ctx.rs:176 static DEFAULT_CAPS=allow_all`, `:305 load_module default Err` único dinámico real `varn-vm/src/exec/host/mod.rs:367 ExecCtx::load_module` sin validación caps.
- `varn-pipeline/src/execute.rs:26 allow_all`, `opts.rs:25 allow_all`, `varn-cli/.../check.rs:14,debug.rs:37,eval.rs:14,repl.rs:67,tester.rs:123 Default=all`, `varn-dap/src/driver.rs:52 allow_all`.
- `varn-cli/src/commands/run.rs:50 allow_all||!has_any_allow=>allow_all`, `:107 net_ports:None nunca`.
- `varn-builtins/src/dispatch/mod.rs:128 capability:None`, `:196 check muerto`, `entry.rs:9 capability existe sin uso`, `varn-contract/.../mod.rs:139,member.rs:220,269 capability_mask:0`.
- `varn-vm/src/exec/host/isolates.rs:257,263,266,274 Vm::new(empty)+with_loader+load sin caps/precompiled`.
- `ffi.rs:21 has_capability("sys.ffi") x12 stringly-typed`, `fs.rs:46 check_fs`, `net.rs:34,49`, `sys.rs:29`.

Impacto: sandbox inútil. Cualquier `vn run/eval/repl/test/check/debug/dap` corre `allow_all`. Capability desconocida permite. Isolate escapa sandbox padre. `ends_with` permite `evil-example.com` por `example.com`. `canonicalize.or(path)` permite bypass con path inexistente.

Fix breaking (ganancia: corrección seguridad, verificable por tests permiso denegado):

- `CapabilitySet::default()=sandbox()`. Borrar `allow_all` como default. `has_capability _=>false`.
- `check_net_connect`: `==` o `ends_with("."+allowed)` + `allowed=="*"`. No `ends_with` crudo.
- `path_matches_any`: si `canonicalize` falla en path o prefijo → `false` (denegar), no fallback.
- `check_fs/net/env`: `None` significa denegar si mask pide paths? Decidir: `None=>true` solo si mask lo permite pero documentar; mejor `None=>false` cuando modo restrictivo explícito. Mínimo: CLI debe poblar `net_ports` y `fs_*_paths` cuando hay flags, hoy `None` siempre.
- `run.rs:50`: borrar `||!has_any_allow=>allow_all`. Sin flags → `sandbox()` o perfil explícito `default-deny + std sin sys`.
- `check/eval/repl/tester/debug/dap`: pasar `sandbox()` o caps mínimas, no `Default`.
- `DummyCtx/StaticInitCtx`: borrar `DEFAULT_CAPS`, exigir `CapabilitySet` explícito en constructor. `fast.rs:55` fallback `Default` → error.
- `NativeOpEntry.capability_mask` + `dispatch entry.capability`: poblar real por módulo (`fs.read`, `net.connect`, etc), `dispatch_runtime_op` debe negar. Borrar `capability:None`.
- `spawn_isolate/fork_for_task`: clonar `capabilities` + `precompiled` padre. Prohibir `Vm::new(empty)` en isolate sin caps.
- `ExecCtx::load_module` host: exigir `check_import Layer` + `has_capability` carga.

Verificación: tests `fs.read denegado`, `net.connect evil-example.com denegado`, `isolate hereda sandbox`, `capability desconocida denegada`, matriz `verify.ps1 -Fast` 4 cuadrantes.

### H2. Runtime re-resuelve por nombre, sin linkado estático

Evidencia:

- `ctx_modules.rs:121 resolve_specifier_from_path`, `exec/modules.rs:4 ModuleResolver::new()` fresco, `loader.rs:22 vs modules/loader.rs:94` firmas incompatibles, `dispatch/modules.rs:15-21 spec_idx const`, `opcode.rs:135 sin ImportDynamic`, `compiler/from_tir/build/exports.rs:42-75 LoadModule+GetProperty Dynamic`, `tir/resolution.rs:5-31 ModuleSlot muerto + assign.rs:183 Unsupported`, `ctx_modules.rs:183-191 loader.load fallback`, `:139-155 doble identidad spec_for + build_module fallback`, `jit_helpers/modules.rs:40 null silencioso, :62 panic!`.

Impacto: TOCTOU compile vs run. Checker valida tipos contra `ExportMap`, runtime hace `GetProperty` por string contra objeto vivo. Cambio disco entre compile y run, versión distinta std, o `build_module` nativo vs `precompiled` divergen sin error. `ModuleSlot` estático muerto → cero verificación slot en linkado. `debug_assert sorted:204-208` solo debug.

Fix breaking (ganancia: corrección + performance — linkado O(1) por slot vs lookup string por import; elimina JIT fallback recompilación):

- Un solo `trait ModuleLoader { load(id)->ModuleInterface }` en `varn-core` o `varn-sem` (Ley1). Borrar `varn-vm/src/loader.rs:22` y `varn-modules/src/loader.rs:94` duplicados, dejar uno canónico. `CompositeLoader` único.
- Emitir `LoadModuleSlot/StoreModuleSlot` con `ModuleId + slot` verificados en compile contra `ExportMap.assign_slots`. Borrar ruta `LoadModule{source}+GetProperty Dynamic` para imports estáticos. Reservar `LoadModule` solo para futuro dinámico con caps.
- Convertir `debug_assert export_names sorted` en error linkado `E_SLOT_MISMATCH` siempre (release también).
- Borrar fallback `build_module(name)` segundo intento por string sin prefijo; una sola identidad `canonical_id_str`.
- Borrar `loader.load` recompilación runtime en `load_module_from_source:183-191` o exigir `source_hash == compile_hash`, sino `E_STALE_MODULE`.
- `jit_load_module_slot null → error`, `jit_store panic → Result`.
- `resolve_specifier_from_path` debe usar mismo `ModuleResolver` inyectado + `check_import Layer`, no `new()` fresco.

Verificación: test `import inexistente falla en compile no en run`, `slot mismatch falla`, `stale module falla`, bench `import` antes/después (lookup string vs slot).

### H3. `Linker` + `GlobalStore` con `UnsafeCell` / `unchecked`

Evidencia: `linker.rs:15-16,32-39,66`, `ctx.rs:32,35,48-49,68,70,251,266`, `globals/mod.rs:33,56,66,88,106,116`, `ctx_modules.rs:127,158,217,233,244`.

Impacto: `&self → &mut` vía `UnsafeCell` es `undefined behavior` si alias + reentrancia (nativo llama `load_module` que muta `modules` mientras iterador vive). `get_unchecked` OOB → UB. `define overwrite` + `resize` silencioso ocultan bugs slot. `fork_for_task share()` alias cross-thread con `Rc` no `Send` → solo funciona por `thread::spawn` con `Vm` nuevo, pero `linker.share()` dentro mismo hilo aliasa.

Fix breaking (ganancia: corrección UB, costo runtime cero con `RefCell` en build dev + `debug` checks; en release mismo layout):

- `Linker { inner: Rc<RefCell<FxHashMap>> }` + `share()->Rc<Linker>` o `Arc<Mutex>` si cross-thread real. Borrar `table_mut(&self)` + `mut_from_ref`.
- `GlobalStore`: borrar `get_by_index_unchecked/set_by_index_unchecked`, usar `get_checked` con error `E_GLOBAL_OOB`. `define` error si duplica salvo `reserve_region`. `set_by_index` error si `idx>=len`, no `resize`.
- `ctx.modules/globals`: acceso vía métodos `borrow/borrow_mut` con guard, no `&*get()` crudo.
- `NATIVE_GLOBALS order` documentar como ABI congelada o generar desde `contract`.

Verificación: `cargo clippy -D missing-safety-doc -D mut_from_ref`, tests `circular TDZ`, `task fork` bajo `VARN_NO_JIT=1` + JIT, miri si aplica a `linker/globals`.

### H4. Hasher aleatorio + orden hash determinan bytecode

Evidencia:

- `artifact.rs:236-248 DefaultHasher`, `compile.rs:119-121 fold FxHashMap values`, `module_precompile.rs:104-118 topo_queue desde iter hash`, `encode.rs:35-53 values() sin sort`, `core/loader.rs:44 bindings hash`, `exports.rs:168,206 iter() copia All`, `package.rs:124 manifest.exports hash`, `tir/ty.rs:133-149 dedup FxHashMap índice secuencial`, `checker/mod.rs:46-73 caches FxHashMap`, `sem/bind.rs:43-59,83-99 FxHashMap`, `graph.rs:10-18 FxHashMap`, `type_impl.rs:241 seen FxHashSet union`, `table.rs:86-93 freeze extend drain`.

Impacto: viola Ley4. Misma entrada → distinto `module_key`, distinto `graph_hash`, distinto orden compilación, distinto `slice`/`absorb`, distinto `export *` ganador en colisión. Suite pasa/falla según corrida. Caché nunca hit entre procesos por `DefaultHasher`.

Fix breaking (ganancia: determinismo byte-idéntico, caché hit; verificación: dos corridas mismo hash):

- `source_fingerprint/module_key` → `fnv1a64` o `xxh3_64` (ya `hash.rs:1-8`, `atom.rs xxh3`). Borrar `DefaultHasher` en `artifact.rs`.
- `graph_hash` → `BTreeMap` sort keys antes fold, o `xor` conmutativo documentado (mejor sort).
- `topo_queue` → `BTreeMap<&str,usize>` o `Vec sort` inicial + `BTreeSet` listos. Orden explícito alfabético.
- `encode.rs from_live` → `keys.sort()` para `classes/interfaces/objects/enums/namespaces/flattened/getters/setters` antes `roots`.
- `core/loader.rs build_core_exports/members` → iterar `core_module_ids` (ya ordenado) + `BTreeMap` para `type_members` o `sort_by name`.
- `exports.rs All` → `sorted keys` antes copia, colisión → error no `skip`.
- `package.rs wildcard` → `BTreeMap` o `sort keys`.
- `Tir TyTable` → aceptar índice local por módulo (bien) pero documentar `dedup` no cruza módulos; no usar `FxHashMap` iter para emitir.
- `Checker` caches `FxHashMap` solo si no afectan orden emisión; auditar `infer_cache/compat_cache` no iterados para codegen (hoy no, pero prohibir `iter`).

Verificación: `VARN_CACHE_DIR=<tmp> vn build` dos veces → `sha256 .vncache` idéntico; `topo` test con 3 módulos independientes orden alfabético.

### H5. Clases foráneas degradan a `Dynamic` en backend

Evidencia: `emit/ty/lower.rs:31-48,107-144,163-174`, `body/scope.rs:286-294`, `body/expr_member.rs:254-266`, `compiler/from_tir/ty.rs:4-51`, `ssa/emit/register_meta.rs:27-59`.

Impacto: viola Ley6. Checker prueba `Class Foo` cross-module, backend emite `Dynamic` → sin layout, sin field check, todo `ByName`, perf cae (no `Ref`/`Int`/`Float` directo, no `NativeShape`), errores campo se van a runtime.

Fix breaking (ganancia: perf runtime + corrección — `Class` foránea con layout real permite `field offset` y `Native` ABI; verificación: bench acceso campo cross-module):

- `NameResolver` debe exponer `foreign_class_id(name,origin)->ClassId` igual que `foreign_enum_id`. `resolve_type_ref` resuelve ambos, no solo enum.
- `emit tables::build` debe incluir `class_defs` foráneas alcanzables (roots desde `imports`), no solo locales. `TirModule.class_defs` ya existe pero `emit_module` solo `prime+build(bind)` local.
- Si tipo no resoluble → error `E_UNKNOWN_TYPE` en compile, no `opaque()`. Reservar `Dynamic` solo para `Declared Dynamic` + `IndexSignature` real.
- `from_tir/ty.rs Class sin tir.class → error`, no `Ref` silencioso. `register_meta conflicto → error`, no `Dynamic`.
- Añadir métrica `DynReason` por módulo en `vn debug -p` para cazar `NotYetSupported` residual.

Verificación: test `import { Foo } from "./foo"; new Foo().x` mantiene `BackendTy::Class` en `tir dump`, bench campo cross-module igual que local.

### H6. `absorb` disperso, ids foráneos crudos

Evidencia: ver §0 cruce ids. `binder_intern.rs:10-12`, `imports.rs:48-50,114-140`, `binder_entry.rs:92`, `type_infer_member.rs:41`, `context.rs:30-86`, `bind.rs:282-313`, `exports.rs:14-45`, `cache_io.rs:58,65,84-85`, `type_queries.rs:130-143`, `stages.rs:81-82`, `resolution.rs:6-43 panic`, `portable/model.rs:5 + encode/decode + codec.rs`.

Impacto: viola Ley2/Ley3. `Symbol.ty` + `members` viajan con ids de otra tabla. Olvidar un `absorb` → `panic index out of bounds` o lectura tipo wrong-index (mismo índice otro tipo). `stages.rs:81 bind.ty_table=checker.ty_table` muta entrada. `try_resolve || ty_table.name` doble fuente oculta stale.

Fix breaking (ganancia: elimina clase bugs orden-dependientes + panics caché; simplifica modelo Ley8):

- Una sola función canónica `rehome_ty(ty: Type, from: &CheckerTyTable, to: &mut CheckerTyTable) -> Type` + `rehome_symbol` en `varn-sem`. Borrar `adopt/absorb/reintern_foreign_ty` dispersos, todos llaman esta.
- `TypeContext::get_*_members` y `BindView::foreign` devuelven ya rehomeados a tabla llamadora, nunca clon crudo. Firma cambia a `(&self, type_table: &mut CheckerTyTable, ...)`.
- Prohibir `bind.ty_table = ...` post-pass. `Checker` trabaja sobre `&mut BindResult` con préstamo, no reemplazo.
- `Atom.resolve` → no panic en Laws nuevas: usar `try_resolve` + error `E_STALE_ATOM` con `source_file + atom dbg`. Mantener `resolve` solo en tests.
- `PortableModule` debe validar `names` contiene todo `Atom` alcanzable (hoy `encode Roots BTreeSet` bien, pero `decode` debe error si falta, no `intern` vacío).

Verificación: test `module_cache_atom_roundtrip` + `shared_interner_origin_module` con caché limpio `VARN_CACHE_DIR=<tmp>`, fuzz `adopt_dependencies` orden inverso mismo resultado.

---

## 2. Hallazgos P1

### H7. Duplicación resolvers/loaders + IO en fase baja (Ley1)

Evidencia: `modules/loader.rs:163,232,144`, `package.rs:252,296,378`, `artifact.rs:271,287,375-376,65,85,25-34,202-229`, `graph.rs:76-80`, `cache_io.rs:44,105`, `resolver_trait.rs:31,52`, `resolver_disk.rs:51-54`, `imports.rs:22,28`, `paths.rs:24-26`, `core/loader.rs:25,41`, `stages.rs:16-33`, `module_precompile.rs:258`, `stdlib_loader.rs:22-24`, `pipeline/resolver.rs:10`, `cli/debug_binder.rs:17,dashboard.rs:15,interact.rs:18`, `vm/loader.rs:22`, `vm/exec/modules.rs:4`, `cli/bench/harness.rs:13,45,68 duplica execute.rs:95`.

Fix: `trait ModuleLoader { load(id: &ModuleId) -> Option<ModuleInterface> }` en `varn-sem` (capa baja, sin disco). `DiskResolver/FilesystemLoader/ProviderLoader/MemoryLoader` solo en `varn-pipeline/cli`. `Binder/Checker` reciben `&dyn ModuleLoader`, nunca `DiskResolver`. Un `Session` único inyectado a `PipelineLoader`, `Vm`, comandos CLI. Borrar `ModuleResolver::new()` fresco en `vm/exec/modules.rs`, borrar `resolve_import_specifier` duplicado en `module_precompile.rs` (llamar `varn_modules::resolver`), borrar `VmFactory` duplicado en `bench/harness.rs` (llamar `boot_machine`).

### H8. Exports con pérdida silenciosa

Evidencia: `resolver/exports.rs:77-106 primer declarator solo`, `:153-177 skip colisión`, `:178-213 Namespace+Named("*")+aplanado ns.sub`, `:107-126 rehome muta out.table`, `binder/imports.rs:195-222 solo dep`.

Fix breaking: `decl_primary_name` debe expandir N declarators o error. `export *` colisión → error `E_EXPORT_COLLISION` con ambos orígenes, no `skip`. Una sola representación namespace (objeto módulo, no aplanado+anidado). Resolver reexports en `binder` (definir símbolos) no solo en colector.

### H9. Capas solo binder

Evidencia: `layer.rs:28-53`, solo `imports.rs:10` llama.

Fix: `check_import` también en `resolve_specifier_from_path` runtime + `build_module_graph` + `DiskResolver::resolve_specifier`. Normalizar spec (`canonical_or_original`) antes `of_module`. `Relative` con `..` fuera root → error, no `Ok` ciego.

### H10. Caché sin hash interfaz + dir global

Evidencia: `pipeline/cache.rs:29-113 solo source_hashes`, `cache_io.rs:18-31 DefaultHasher`, `artifact.rs:221-229 global ~/.varn/cache`, `:47-87 prune mtime`, `:370-382 sin fsync`.

Fix: `source_hashes` + `interface_hash = fnv(postcard(PortableModule))` por módulo; `load_cached_graph` invalida si interfaz cambió aunque fuente igual (caso cambio dep transitiva). `module_key` con `fnv` determinista + `project_root_hash` prefijo. `write_artifact_file` + `fsync(file)+fsync(dir)` en Unix.

### H11. Host ABI sin versión + transmute + globales

Evidencia: `native.rs:83,130`, `native_abi.rs:6-101`, `jit-abi/lib.rs:115-243 sin VERSION`, `proto/definition.rs:50-113 skip`, `native_entry.rs:78-95 sig+epoch`, `contract/fast.rs:55,72 DummyCtx`, `mapping.rs:189 u8 7 slots`, `dispatch/mod.rs:36 from_raw_parts + OnceLock<Mutex>`, `rt/lib.rs:3,46 Box::leak,65 exit(1)`, `globals:SILENT AtomicBool`, `timer WHEEL OnceLock<Mutex>`, `channel REGISTRY OnceLock`, `sqlite DB_POOL Mutex<Vec>`, `profile TASK_STATS static`, `clif_link thread_local CURRENT_CTX/EPOCH/COMPILED + NEXT_EPOCH AtomicU64`, `pumps thread_local PUMPS/HOSTS`.

Fix breaking: `JitHelpers::VERSION: u32` + `assert_version` en `table_build.rs:7-33` (ya `offset_of+probe`, añadir número). `NativeOpEntry.capability_mask` real. `fast_wrap` sin `DummyCtx` fallback → error si decode falla. `signature_token` `u8` → `u16` o error explícito `>7 args no fast`. `__VARN_OPS_START/END from_raw_parts` documentar safety + test `iter_native_ops` cuenta. `Box::leak str_concat` → devolver `StrResult` con owner o interner, no leak. `SILENT AtomicBool` → pasar en `NativeCtx`, no global.

---

## 3. Hallazgos P2

### H12. `unwrap_or(Dynamic)` encubre `None`

`member_named.rs:55`, `member_type.rs:158`, `recorder.rs:114`, `output.rs:99-110`, `type_impl.rs:125-148`, `emit/module_globals.rs:103`.

Fix: `enum SymTy { Known(Type), Unknown }`. `Unknown` → error o `Dynamic(Unknown)` con razón distinta `Declared`. No mezclar.

### H13. `compat` re-resuelve `Atom→String` por llamada

`compat/resolve.rs:8-15`, `nominal.rs:24-40,178-213`, `type_queries.rs:26-44,120-128`.

Fix: cache `FxHashMap<Atom, Arc<str>>` por `Checker` o comparar `Atom(u128)` directo (ya `xxh3` estable) + origen `Atom` directo, sin `resolve` string salvo diagnóstico.

### H14. Ciclo runtime binder↔resolver

`binder_entry.rs:14,30,50`, `imports.rs:38`, `context.rs:31` vs `resolver_parse.rs:52-56`, `resolver_trait.rs:160-161,169`, `cache_io.rs:78-81`, `stages.rs:22-24`, `type_queries.rs:162-166`.

Fix: ver H7. `Binder` no importa `ImportResolver` de `varn-sem` que implementa `DiskResolver` alto; mover trait a `varn-core`.

---

## 4. Plan corrección detallado (orden + commits atómicos Ley9)

Cada paso = un commit revertible, mensaje sin "y". Validación iteración `cargo build --profile quick --bin vn` + `.\scripts\verify.ps1 -Quick` (dev-checkout/JIT). Matriz completa `-Fast` (release, 4 cuadrantes std dev-checkout/@embedded × JIT/VARN_NO_JIT=1) solo antes de commit. Si contradicción código vs plan, código manda + registrar bug diseño (AGENTS.md §0).

### Fase A — determinismo + seguridad (P0, sin cambio semántico salvo fail-closed)

A1. `artifact hasher determinista` — tocar `modules/src/artifact.rs:236-248` → `fnv1a64`/`xxh3_64`, `cache_io.rs:18-31` igual. Borrar `DefaultHasher`. Tests: `module_key` estable entre procesos (dos `vn` distintos mismo output). Ganancia: caché hit. Verificación: `VARN_CACHE_DIR=<tmp1>/<tmp2>` mismo `module_key`.

A2. `topo + graph_hash determinista` — `module_precompile.rs:104-118 BTreeMap/sort`, `compile.rs:119-121 BTreeMap fold`, `encode.rs:35-53 sort keys`, `core/loader.rs:44 sort`, `exports.rs:168,206 sort`, `package.rs:124 sort`. Tests: 3 módulos independientes orden alfabético estable, `graph_hash` igual dos corridas.

A3. `capabilities fail-closed` — `capabilities.rs:28,62` (`Default=sandbox`, `_=>false`), `native_ctx.rs:176` borrar `DEFAULT_CAPS`, `fast.rs:55` error, `run.rs:50,107` (`sandbox` + `net_ports`), `check/eval/repl/tester/debug/dap/driver` caps mínimas, `isolates.rs:257-274` clonar caps+precompiled, `dispatch capability_mask` real, `path_matches_any` denegar si no canonicaliza, `check_net_connect` suffix con punto. Tests permiso denegado. Breaking declarado: sí, cambia default. Ganancia: seguridad. Borrado: `allow_all` default.

A4. `linker/globals checked` — `linker.rs RefCell`, `globals/mod.rs` borrar `unchecked`, `define/resize` errores. Tests TDZ + task fork.

### Fase B — frontera tipada portable (P0, breaking)

B1. `rehome canónico` — nuevo `varn-sem/src/rehome.rs: rehome_ty/rehome_symbol`. Migrar `binder_intern/imports/type_infer_member/context/BindView/exports/rehome/reintern_foreign_ty/cache_io/stages` a llamar esta. Borrar `adopt` dispersos. Prohibir `bind.ty_table=` (cambiar `stages.rs:81` a préstamo `&mut`). Tests caché orden inverso.

B2. `foreign_class_id + emit class_defs foráneas` — `NameResolver: foreign_class_id`, `lower.rs:31-48` resolver clase igual enum, `tables::build` incluir alcanzables foráneas, `from_tir/ty.rs` + `register_meta` error en vez `Dynamic/Ref` silencioso. Métrica `DynReason` en `debug -p`. Bench campo cross-module.

B3. `ModuleSlot linkado` — `compiler/from_tir/build/exports.rs` emitir `LoadModuleSlot/StoreModuleSlot` verificados contra `ExportMap`, `assign.rs:183` implementar (borrar `Unsupported`), `ctx_modules.rs:204` assert → error `E_SLOT_MISMATCH`, borrar doble identidad + `loader.load` fallback o exigir hash. Test slot mismatch + stale.

### Fase C — capas + resolvers únicos (P1)

C1. `ModuleLoader único bajo` — mover trait a `varn-core` o `varn-sem` mínimo, `DiskResolver/Filesystem/Provider/Memory` arriba, inyectar `Session` único a `PipelineLoader/Vm/CLI`. Borrar `new()` fresco, `resolve_import_specifier` duplicado, `VmFactory` duplicado. Test fase sin IO (`Binder` con `MemoryLoader`).

C2. `exports estrictos` — N declarators, colisión error, una forma namespace, reexports en binder.

C3. `check_import ubicuo` — runtime + precompile + resolver. Normalizar antes `of_module`.

C4. `caché interfaz + dir proyecto` — `interface_hash`, `project_root_hash`, `fsync`.

C5. `JitHelpers::VERSION + host caps` — `table_build assert`, `capability_mask` real, borrar `Box::leak/SILENT` globales.

### Fase D — higiene (P2)

D1. `SymTy Known/Unknown`, D2. `compat Atom directo`, D3. `Atom.resolve` → `try_resolve + E_STALE_ATOM`.

### Checklist por commit (AGENTS.md §4)

- [ ] compilo módulo solo con interfaz portable deps
- [ ] ningún id interno en artefacto serializado (salvo `PortableModule` con `names+types` validados)
- [ ] orden bindeo/interning no cambia resultado
- [ ] fase testeable sin VM/sin IO
- [ ] dos corridas bytecode idéntico
- [ ] sin global mutable nuevo (dueño + reset declarados)
- [ ] sin `_ =>` que oculte variante

### Puerta validación (AGENTS.md §5)

`.\scripts\verify.ps1 -Fast` 4 cuadrantes + `VARN_CACHE_DIR=<temp>` limpio para aislar frontera módulo (Ley2). `cargo check` detecta rotura std bundle (`varn-cli/build.rs`). Criterio: `PASSED:N FAILED:0 ALL TESTS PASSED` x4.

---

## 5. Qué se borra (Ley8, sin adaptadores)

- `DefaultHasher` en `artifact.rs` / `cache_io.rs`.
- `allow_all` como `Default`, `DEFAULT_CAPS`, `DummyCtx` fallback silencioso.
- `table_mut(&self)/mut_from_ref`, `get_unchecked/set_unchecked`, `resize` silencioso.
- `adopt/absorb/reintern` dispersos → una `rehome`.
- `LoadModule+GetProperty Dynamic` para estáticos → `ModuleSlot`.
- `loader.load` recompilación runtime sin hash.
- Doble identidad `spec_for` + `build_module` fallback string.
- `ModuleLoader` duplicado (`vm` vs `modules`), `ModuleResolver::new()` fresco, `resolve_import_specifier` duplicado, `VmFactory` duplicado.
- `export * skip` silencioso + namespace aplanado duplicado.
- `JitHelpers` sin versión, `capability:None/mask:0`, `Box::leak`, `SILENT` global.
- `unwrap_or(Dynamic)` encubridor.

Sin flags duales, sin `Carrier` extra, sin segunda ruta resolución. Migración por pasos donde cada paso es parte diseño final.

---

## 6. Estado implementación (2026-10-09, implementado + verificado)

Suite 4 cuadrantes con perfil `quick`: `PASSED: 2287 FAILED: 0 ALL TESTS PASSED` en dev-checkout+JIT, dev-checkout+No-JIT, @embedded+JIT, @embedded+No-JIT. `cargo check --workspace --all-targets` limpio. `cargo clippy` sin errores nuevos (solo `collapsible_if` preexistentes). `gen-contract-tables --check` limpio. Commits atómicos en `main` (Ley9). Matriz `release -Fast` pendiente.

### Implementado

- A1: `modules/src/artifact.rs:236-248` `DefaultHasher` → `fnv1a64` local en `source_fingerprint` + `module_key` (hashea `id.as_str()`).
- A2: `pipeline/compile.rs:119` `graph_hash` con keys sorted. `module_precompile.rs:104-134` topo con `zeros.sort()` + `newly_ready.sort()` + `cycle_nodes.sort()`. `resolver/exports.rs` copia `export *` (con y sin alias) con keys sorted. `binder/core/loader.rs` `build_core_exports` con keys sorted. `modules/package.rs` wildcard con keys sorted. `encode.rs` sin cambio: `slice()` ya usa `BTreeMap` (determinista verificado en `slice.rs:15-22,173-178`).
- A3 parcial: `capabilities.rs` `Default=sandbox()`, `has_capability _=>false`, `check_net_connect` exige `==`, `*`, `dominio.` o `.sufijo` (no `ends_with` crudo), `path_matches_any` deniega si no canonicaliza. `native_ctx.rs:176` `DEFAULT_CAPS=sandbox()`. `cli/run.rs` parse `host:port` → `net_hosts`+`net_ports`. `cli/eval.rs,repl.rs,debug.rs` `allow_all()` explícito (herramientas dev preservan conducta). `check.rs` mantiene `Default` (ahora sandbox, correcto: `no_run`). `dap/driver.rs` ya era `allow_all()` explícito. `vm/isolates.rs` hereda caps padre por valor clonado (`CapabilitySet: Send`, `Rc` no cruza `spawn`; `precompiled` no heredado por `Rc:!Send`, fallback por loader — documentado). Desviación consciente: `run.rs:50` mantiene `allow_all` por defecto sin flags para no romper suite/`verify.ps1` (llama `vn run` sin flags); fail-closed total de `run` queda como A3b pendiente con anotación caps en suite.
- A4: `vm/linker.rs` `Rc<UnsafeCell>` → `Rc<RefCell>`, borrado `table()/table_mut()/mut_from_ref`. `vm/globals/mod.rs` `set_by_index` ya no `resize` (assert `E_GLOBAL_OOB`), `get/set_by_index_unchecked` con assert en vez de `get_unchecked` (sin UB).
- B1 parcial: `varn-sem/src/rehome.rs` (`rehome_ty`, `rehome_ty_with_origin`, `rehome_symbol`, `canonicalize_lang_named`) + `lib.rs: pub mod rehome`. Migrados `checker/type_queries.rs:reintern_foreign_ty`, `binder/type_infer_member.rs:reintern_member_type` e `imports.rs:135-140` (con `with_origin`) a la canónica. Resto call sites (`binder_intern`, `context`, `exports`, `cache_io`, `stages:81`) pendientes.
- B2: `sem/output.rs: ForeignClass{name,origin,fields,methods}` + `Desugarings.foreign_classes`. `checker/foreign_classes.rs: collect_foreign_classes` (BTreeMap, `reintern_foreign_ty`, guards representación: `classify==Class`, sin padre, no lang-name, no builtin/intrinsic). `stages.rs` puebla `foreign_classes` antes de `foreign_enums`. `emit/ty/resolve.rs: foreign_class_id` en trait + `NoNames`. `emit/tables/index.rs: foreign_classes` en `NameIndex` + `parent_class_id` (local, foráneo con origin, fallback por nombre) + `foreign_parent_info`, build foráneas primero (Root) y luego locales con herencia enlazada. `emit/ty/lower.rs: resolve_type_ref` resuelve `foreign_class_id` antes que `foreign_enum_id`. `emit/tables/classes.rs: build_classes/with_parents/one_class` aceptan `foreign_parent_infos`, chain corta en foráneo, `Ancestry::Local` a padre foráneo (sin duplicar prefix).
- H2 parcial: `vm/ctx_modules.rs:204` `debug_assert sorted` → error `E_SLOT_MISMATCH` en release también.
- H9 parcial: `vm/exec/modules.rs: resolve_specifier_to_id` aplica regla seguridad `Runtime→solo-Std` (`E_LAYER`). Matriz completa no aplicada en runtime porque el prelude del compilador carga `core:` desde módulos User sin pasar por binder (falso positivo verificado en `tests/41-advanced-enums.vn`).
- H11 parcial: `jit-abi/lib.rs: JIT_HELPERS_VERSION=1`, re-export en `jit/lib.rs`, assert en `vm/jit/table_build.rs`. `contract/fast.rs` sin cambio: fast path solo escalares no-falibles, `DummyCtx` ahora sandbox sin efecto en decode.
- Hallazgo nuevo B4 (verificador TIR): `tir/verify/coherence/ops.rs: check_binary` eximía solo comparaciones y `Dynamic`; `Instanceof`/`In` heterogéneos fallaban (`e instanceof Error` con ambos lados `Class` distintos) enmascarado antes por `Dynamic`. Ahora exentos con check `e.ty==Bool`. Sin esto H5 es imposible.
- R2: `rehome::canonicalize_lang_named`: `Named` lang-name + origen == `core_module()` → `Builtin`/`Primitive`. Cableado en `rehome_ty`, `rehome_ty_with_origin` e `imports.rs` (tras `with_origin`). Los tipos nativos ya no viajan como `Named` foráneo.
- R3: `BindResult::classify_decl` (`DeclKind`: prioridad `SumType > Enum > Class > Interface > Namespace`, `debug_assert` unicidad para builds dev). Ambos colectores la usan: clases exige `Class`, enums exige `Enum|SumType`. Hallazgo raíz medido: `bind_enum` (`binder/decl_values/type_decls.rs:46,268,272`) registra cada enum en TRES mapas (`sum_type_variants` + `classes` + `enums`); el colector viejo leía mapas espejo y la primera versión de `classify` tenía bug last-wins (devolvía `Class` para `Result`, rompiendo `try`; verificado con `VARN_DEBUG_FOREIGN` temporal, ya retirado). Con first-wins, `Result`/`Option` van solo a enum y `try`/`match` vuelven precisos.
- R4: `modules::is_prelude_module` (misma lista que usa `emit/prelude.rs`). `pipeline/layer_link.rs: verify_synthetic_imports` en linkado (entry + deps): arista TIR no declarada en AST debe ser prelude. `binder/imports.rs: check_reexport_layer` (`export...from` sin check antes; negativo verificado → `VN2010`). Runtime mantiene `Runtime→solo-Std` como backstop.
- H8: `decl_primary_name → decl_export_names` (N declarators; antes pérdida silenciosa desde el 2º). `ExportMap.collisions` + first-wins + `E_EXPORT_COLLISION` en linkado (entry, deps, stdlib). Hallazgo raíz medido: `export *` sin alias NO emitía `TirExport` (`emit/imports_exports.rs` sin rama `All`, suite sin cobertura) → slots siempre null (preexistente, expuesto al probar H8a). Fix: `pipeline: emit_and_compile` dividido en `emit_tir` + `inject_star_reexports` + `compile_tir` (+ `tir_import_sources`; wrapper borrado; `varn-tir` dep directa de pipeline), que materializa star vía `reexport_from` con el mecanismo existente.

### Regresiones encontradas y causa raíz (evidencia para H5 completo)

- 137-class-build-order: upcast `const asBase: ImportedBase = sub` fallaba (`Class(10)` vs `Class(1)`) porque subclase local usaba `Ancestry::Foreign` sin link. Fix: enlace padre foráneo (arriba).
- 11-errors `ve instanceof Error → false` en runtime: identidad clases duplicada por módulo (Ley6). Patrón codebase (`instance_of_name`, `lower_is`, `match_pattern`) ya usa `TypeTest` solo local + dinámico para foráneo; `expr_binary` se alineó a ese patrón (se revirtió resolución foránea a `TypeTest`). Identidad runtime cross-module unificada queda pendiente (requiere registro global clases o checks por nombre en runtime).
- 18-ranges `slot 3 OOB` en `Range`: stub core de tipos nativos tiene `class entry` pero runtime usa representación nativa. Fix: guard `is_lang_type_name` en `collect_foreign_classes`.
- 109-try-operator `int = Class`: `Result` es sum-type con `class entry`; se colectaba como clase y ganaba a enum. Fix: guards sum-type/enum en `collect_foreign_classes`.

### Pendiente (no tocado)

R1 + H2 completo (registro global clases + `ModuleSlot` emitido, §8 con plan F1-F5), H7, H8 resto (namespace único: rama alias aplana `ns.sub` además del objeto — unificar a una representación), H9 resto (`Relative` con `..` fuera de root), H10 (`interface_hash`, `project_root`, `fsync`), H11 (`capability_mask`, `Box::leak`, `SILENT`), H12-H14, A3b (`run` fail-closed), B1 resto call sites, `from_tir/ty.rs` + `register_meta` error en vez de `Dynamic` (B2 resto), métrica `DynReason`, `E_STALE_ATOM`. Guards representación en `collect_foreign_classes` (lang-name, sin-padre, builtin) se quedan hasta R1: no son mecanismos duplicados, son restricciones de representación sin registro global. Guards representación en `collect_foreign_classes` (lang-name, sin-padre, builtin) se quedan hasta R1: no son mecanismos duplicados, son restricciones de representación sin registro global.

---

## 7. Rediseño raíz de las regresiones (breaking, Ley8/Ley10)

Las 5 regresiones halladas al implementar B2/H9 son parches sobre una causa común: **no hay identidad única cross-module** (cada módulo duplica layouts, runtime re-resuelve por nombre). Todo lo de abajo es breaking y declara (a) ganancia, (b) verificación, (c) borrado.

### R1. Identidad clases duplicada → `instanceof` falso, riesgo slots

Parche actual: copiar `ClassInfo` foránea por módulo + `instanceof` dinámico para RHS foráneo.
Raíz: **registro global de clases en linkado**. En `build_module_graph`/`compile`, asignar `ClassId` global (internado `(ModuleId, name)`, orden determinista) y UNA tabla `ClassInfo` compartida por programa; los protos referencian ids globales; la VM lleva la tabla compartida (`TypeTest`, field slots y `assignable` consultan el registro, nunca copias locales).
(a) Corrección identidad + perf (slot access en vez de `ByName` en cross-module). (b) `ve instanceof Error == true` con `TypeTest` estático, suite 4 cuadrantes, bench campo cross-module == local. (c) Borrar `ForeignClass`, copias foráneas en `tables/index.rs`, `parent_class_id`/`foreign_parent_info`, fallback dinámico en `expr_binary`.

### R2. Tipos nativos con stub clase (`Range` → slot OOB)

Parche actual: guard `is_lang_type_name` en el colector.
Raíz: **canonización en binder**: `Named("Range","core:types/range")` → `Builtin(Range)` (igual para todo `BuiltinType`/primitivo con origen core conocido). Una sola representación (Ley8); `lower_builtin` ya mapea a `BackendTy` correcto/`Dynamic` donde no hay variante. (a) Elimina clase de bugs stub-vs-nativo. (b) Suite + assert `ForeignClass` nunca contiene lang-name. (c) Borrar rama `Named` para lang-names en `lower`/`resolve_type_ref`.

### R3. Sum-type registrado como clase+enum (`Result`)

Parche actual: guards de exclusión en un colector.
Raíz: **query canónica única** `classify(name, origin) -> Class|Enum|Sum|Builtin` en `varn-sem` (Ley6), usada por AMBOS colectores; `emit` con `debug_assert` de registro exclusivo (un tipo, un kind). (a) Imposibilita la dualidad por construcción. (b) Test colector con `Result`/`Option`/clase real. (c) Borrar condiciones espejo en `foreign_enums.rs`/`foreign_classes.rs`.

### R4. Capas en runtime vs prelude (check debilitado a subset)

Parche actual: solo regla `Runtime→solo-Std` en `resolve_specifier_to_id`.
Raíz: **verificación en linkado, no en runtime**. El grafo cerrado (`build_module_graph`, que SÍ distingue aristas sintéticas de prelude/intrínsecos de imports de usuario) verifica la matriz completa y sella `layer_digest` en el artefacto; el runtime confía en el grafo sellado + hashes interfaz y no re-verifica strings (el check runtime es TOCTOU por diseño: el fichero puede cambiar entre check y load). (a) Sin falsos positivos, sin TOCTOU. (b) Test prelude `core:` desde User pasa, `runtime:` directo desde User falla en linkado. (c) Borrar `E_LAYER` runtime por string.

### R5 (marco). Linkado cerrado H2

R1+R4 son casos de H2: emitir `ModuleSlot` verificado contra `ExportMap` en linkado + `interface_hash` en caché = closed-world. El runtime pasa a lookup en tabla, cero strings, cero re-resolución. Orden ejecutado: R2+R3 (commit `5bbe91db`) → R4 (`verify_synthetic_imports` + `check_import` en reexports) → H8 (star materializado + colisiones) → R1/H2 (§8).

---

## 8. R1/H2: plan de implementación (alcance medido, F0 ejecutado)

Mecanismo probado con evidencia (no especulación):

- `compiler/from_tir/build.rs:117-134` `TypeTest` baja a `global_load(cname)` + `HirBinOp::Instanceof`: el `instanceof` runtime compara IDENTIDAD de objeto clase (`advanced.rs:125-132` camina `superclass` comparando `c.id == cls.id`), resuelto POR NOMBRE en los globals del módulo actual.
- `compiler/from_tir/build/class_def.rs:13-23` cada módulo ejecuta `MakeClass { name, super_class }` propio (`exec/class.rs:8-11` `op_class` crea SIEMPRE fresco vía `new_rc`, sin consultar ningún registro).
- Dos mecanismos de creación conviven (Ley8): nativas por registro (`contract/mod.rs:116-122` `get_class`/`register_class` en `heap.intrinsic_classes`, singletons por nombre) vs fuente siempre-fresco. No unificar a ciegas por `(module,name)`: la frescura por evaluación es LOAD-BEARING para REPL/eval (redefinir `class Foo` por snippet debe crear objeto nuevo; internar por `(module,name)` acumularía métodos entre snippets).
- `compiler/from_tir/ty.rs:33-36` el `ClassId` local muere en la frontera TIR→SSA (`HirType::Class` por nombre); el slot fixo (`GetFixedField`) se congela contra el `ClassInfo` local.
- F0 medido (instrumento `VARN_DEBUG_CLASS` temporal, ya retirado): `Error#4, TypeError#5...` vía `new_native_rc` UNA vez; `ValidationError#24` vía `new_rc` UNA vez; cero `INSTANCEOF_MISS`; t4 `true`. Creación única por VM (linker/evaluación única funciona); heaps no cruzan threads (`SendValue`); `freeze` rechaza módulos con clases (`module_freeze.rs:70-71` → recargan normal) y `thaw` preserva el `Rc` (`thaw_export: Class`, misma identidad). Identidad runtime SANA bajo evaluación única; el t4-falso histórico fue artefacto de compile-time (ids `ClassId` duplicados entre tablas por módulo), ya fijado con `TypeTest` local-only + dinámico.
- Restricción que queda: sin registro global, la unicidad depende de "un módulo se evalúa una vez" (linker) + frescura por evaluación (REPL). R1 completo = mover esa garantía de runtime-accidental a linkado-explícito (F1-F5).

Fases (cada una verde + commit):

- F1: retener `TirModule.classes/enums` en pipeline (`emit_and_compile` ya devuelve imports; extender a retener tablas por módulo; sin cambio de conducta).
- F2: `ClassRegistry` en `varn-tir` o crate nuevo bajo: `intern((module,name)) -> GlobalClassId` determinista + tabla única; construir en linkado desde las tablas retenidas; test unitario standalone.
- F3: `from_tir` mapea `ClassId` local→global al compilar cada módulo (necesita registro construido ANTES de compilar: reordenar `module_precompile`: emitir todos → registro → compilar todos). `FunctionProto` lleva ids globales (breaking formato + caché: invalida por `BUILD_FINGERPRINT`, ya automático).
- F4: VM: `ExecCtx` con registro compartido (`Rc`); `TypeTest`/`GetFixedField`/vtable consultan registro; `MakeClass` unifica shapes por id global (un solo layout por clase = Ley6); borrar copias foráneas (`ForeignClass`, `parent_class_id`, `foreign_parent_info`) y fallback dinámico en `expr_binary` (Ley8).
- F5: `ModuleSlot`: emitir `LoadModuleSlot/StoreModuleSlot` verificados contra `ExportMap` + `interface_hash` en caché; borrar `LoadModule+GetProperty Dynamic` para estáticos y `loader.load` fallback sin hash.

Verificación F4/F5: t4 `true` con `TypeTest` estático, suite 4 cuadrantes, bench campo/método cross-module == local, caché limpio `VARN_CACHE_DIR=<tmp>`.
