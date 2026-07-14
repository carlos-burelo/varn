# Arquitectura del Compilador (`varn-opt` + `varn-backend`)

> **No existe ningún crate `varn-compiler` ni `varn-ir`.** Este documento los
> describía hasta 2026-07-13. La generación de bytecode vive en **`varn-opt`**, y
> los post-passes sobre el bytecode ya emitido en **`varn-backend`**.

El compilador transforma el TypedAST + anotaciones de `varn-checker` en un
`FunctionProto` (bytecode + constant pool) ejecutable por `varn-vm`.

---

## 1. Pipeline real

```text
TypedAST + anotaciones (varn-checker)
        │
        ▼  hir::lower::lower_program
      HIR
        │
        ▼  hir::inline::run
   HIR (inlineado)
        │
        ▼  ssa::build
      SSA
        │
        ▼  passes::optimize   (punto fijo, ver §3)
   SSA optimizado
        │
        ▼  ssa::emit
  FunctionProto (bytecode + constant pool)
        │
        ▼  varn_backend::run_post_passes
  FunctionProto final
```

Punto de entrada: `varn_opt::compile_module()` / `varn_opt::compile()`
(`crates/varn-opt/src/lib.rs`). `varn-pipeline` lo llama en la fase `compile`; la
fase `optimize` que reporta `vn bench` corresponde a los post-passes de
`varn-backend`.

---

## 2. HIR

`hir::lower` aplana el AST tipado a una representación intermedia de alto nivel,
donde ya se resolvieron scopes, extensiones y azúcar sintáctico.

`hir::inline` corre inlining sobre el HIR antes de bajar a SSA.

Inspección: `vn debug -p hir <archivo>`.

---

## 3. SSA y passes

`ssa::build` construye la forma SSA (`SsaFunc`, bloques + terminadores).
`ssa::verify` valida la forma. `ssa::emit` baja SSA a bytecode.

`passes::optimize` (`crates/varn-opt/src/passes/mod.rs`) corre un **bucle de punto
fijo** — repite hasta que ningún pass reporta cambios, con tope de 100 iteraciones:

| Pass | Qué hace |
|------|----------|
| `tco` | Tail-call optimization |
| `const_fold` | Plegado de constantes (debe coincidir bit a bit con intérprete y JIT — ver `varn-core/src/numeric.rs`) |
| `fixed_fields` | Convierte accesos por nombre en accesos por slot cuando el checker conoce la shape |
| `dce` | Eliminación de código muerto |
| `cfg` | Simplificación y compactación del grafo de control |

Inspección: `vn debug -p ssa <archivo>`. Traza del compilador: `VN_OPT_TRACE=1`.

---

## 4. Post-passes de bytecode (`varn-backend`)

`varn_backend::run_post_passes(&mut proto)` recorre además, recursivamente, cada
`PoolEntry::Function` anidado:

- `regalloc_post::optimize_function` — asignación de registros sobre el bytecode ya
  emitido, apoyada en `liveness`.
- `slot_kinds::infer` — infiere el tipo de cada slot de registro.

`slot_kinds` es lo que alimenta el `register_meta` del que depende el JIT para
saber qué registros puede mantener sin flush. Cambiar la anchura de un slot aquí y
no reflejarlo allí produce corrupción silenciosa en el código generado.

---

## 5. Slots y registros

La VM es register-based. El compilador asigna slots estáticos en compile-time: el
nombre `"x"` no existe en el bytecode, solo su índice. Acceso O(1) por offset sobre
`registers[frame.base + slot]`.

### Upvalues

Si una función anidada referencia una variable del padre, se promueve a upvalue. Al
cerrar el frame padre, `CloseUpvalue` mueve el valor del registro al heap.

---

## 6. El principio del backend: los tipos llegan al bytecode

Cuando el checker sabe el tipo, el bytecode lo aprovecha. Los opcodes tienen
variantes tipadas que se saltan la comprobación de tipos en runtime:

- Genéricos: `Add`, `Sub`, `Mul`, `Lt`, `Eq`, …
- Enteros: `AddInt`, `SubInt`, `MulInt`, `LtInt`, `GtInt`, `EqInt`, …
- Floats: `AddFloat`, `SubFloat`, `MulFloat`, `LtFloat`, `EqFloat`, …
- Inmediatos: `AddImm`, `SubImm`

Lo mismo para el acceso a propiedades: si el checker conoce la clase, el pass
`fixed_fields` emite `GetFixedField slot` / `SetFixedField slot` en lugar de
`GetProperty "nombre"`, y el JIT los baja a un único load/store con offset
constante.

Hay **134 opcodes** (`crates/varn-core/src/opcode.rs`). No llevan prefijo `Op`.

---

## 7. Clases

- **Fixed fields**: ver arriba.
- **Vtable**: `instancia.metodo()` con clase conocida despacha por índice de vtable
  en lugar de por lookup de nombre. La versión de la vtable se cachea en el IC
  (`vtable_ver`) para invalidar entradas si la clase muta.

---

## 8. Control de flujo y back-patching

Los saltos se emiten hacia un destino provisional, se guardan en listas de
pendientes y se parchean al conocer la dirección final (`break`, `continue`,
`if/else`).

`finally` se **inlinea**: si aparece un `return` / `break` dentro de `try` o
`catch`, el bytecode del `finally` se inyecta antes del salto, sin desenrollado en
runtime.

---

## 9. Desugaring

- **Destructuring**: `const { a, b, ...rest } = obj` → temporal + accesos por
  propiedad + `ObjectRest` para el resto.
- **Enums**: `Option.Some(val)` → constructor que emite la variante con tag y
  payload.
- **Namespaces**: `namespace T { export function f() {} }` → objeto con las
  exportaciones.
- **Extensiones**: `"hola".trim()` donde `trim` es extensión de `str` → llamada
  directa a la función de extensión, sin lookup de propiedad.
- **Decoradores**: `@deco function foo() {}` → se compila `foo`, se llama
  `deco(foo, ctx)` con `ctx = { name, kind, isStatic }`; si el decorador devuelve un
  valor, reemplaza a `foo`.
- **`using` / disposables**: `using db = connect()` registra `db` según la
  profundidad del bloque; cualquier escape (`return`, `break`, `continue`) inyecta
  el `dispose()` antes del salto.

---

## 10. Constant pool

Los literales se deduplican en el `constant_pool` del `FunctionProto` y las
instrucciones los referencian por índice. Las funciones anidadas se guardan como
`PoolEntry::Function`.

---

## 11. Inspección

```
vn debug -p tokens|ast|hir|ssa|bytecode|types|symbols|consts|scope <archivo>
VN_OPT_TRACE=1 vn run <archivo>      # traza del compilador
```

Ver [CLI_REFERENCE.md](CLI_REFERENCE.md).
