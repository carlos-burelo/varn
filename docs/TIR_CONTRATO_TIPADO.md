# TIR — contrato tipado (estado código)

Emite: `varn-emit::emit_module` (`crates/varn-emit/src/lib.rs:33-40`): `(program, ast_arena, bind, expr_table, call_mappings, desugar) -> TirModule`.

## `TirModule` (`crates/varn-tir/src/module.rs:119-135`)

`source_file, imports, exports, types: TyTable, classes, class_origins, enums, signatures, functions: Vec<TirFunction>, globals: Vec<BackendTy>, global_names, class_defs, top_level: TirFunction`.

## `TirFunction` (`module.rs:6-22`)

`name, sig: SigId, params: Vec<BackendTy>, return_ty: BackendTy, locals: Vec<BackendTy>, body: Vec<TirStmt>, has_this, this_class?, is_async, is_generator, has_rest, force_inline`.

## `TirExprKind` (`node.rs:96-245`) — sin `Try`

Lits (`Int/Float/Bool/Str/Char/Null/Decimal/BigInt`), `Var`, `Binary`, `Unary`, `Field`, `Index`, `Call`, `MethodCall`, `Assign`, `ArrayLit`, `TupleLit`, `ObjectLit`, `RecordLit`, `Await`, `Yield`, `Discriminant`, `VariantPayload`, `TypeTest`, `Cast`, `Checked`, `Closure`, `New`, `MakeVariant`, `Select`, `Seq`, `ObjectKeys`, `IterInit`, `SuperCall`, `SuperMethodCall`, `RangeLit`, `ObjectRest`, `ExtensionCall`. `Try` no es expresión.

## `TirStmt` (`node.rs:269-274`)

`Throw(TirExpr)` y `Try { body, catch_local: LocalId, catch_body }`. Sin `finally` en TIR.

## `BackendTy` (`ty.rs:57-80`) — 20 variantes

`Int, Float, Bool, Char, Str, Bytes, Decimal, BigInt, Array(TyId), Map(TyId,TyId), Dict(TyId), Set(TyId), Tuple(TyListId), Class(ClassId), Enum(EnumId), Fn(SigId), Nullable(TyId), Void, Never, Dynamic(DynReason)`.

`DynReason` (`ty.rs:36-54`), 9 variantes: `HostBoundary, Union, IndexSignature, Declared, Callable, Awaitable(TyId), Yields(TyId), Shape(ShapeId), NotYetSupported`.
