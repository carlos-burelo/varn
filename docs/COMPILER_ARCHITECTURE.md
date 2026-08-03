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

`hir::inline` corre inlining sobre el HIR antes de bajar a SSA. Acepta
funciones de módulo cuyo cuerpo se reduce a **una** expresión — directamente
`return <expr>`, o una secuencia recta de `let` que se pliega dentro de él
(`single_expression_body`), siempre que cada local se lea a lo sumo una vez o
su inicializador sea un literal/parámetro.

Los candidatos se indexan por el **global cualificado**
(`<source_file>::<nombre>`), que es como los nombra `lower::global_binding` en
el sitio de llamada. Indexarlos por el `HirFunction::name` pelado hacía que la
búsqueda nunca acertara y el pass no inlineara nada en absoluto.

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
| `algebraic` | Identidades con **un** operando conocido (`x + 0`, `x * 1`, `n - n`). El set de float es mucho menor que el de int: `x + 0.0` no es identidad (`-0.0`), ni `x * 0.0` es `0.0` (infinitos → NaN) |
| `cse` | Eliminación de subexpresiones comunes, **local a cada bloque**. Dos tablas: valores puros (nunca se invalidan) y lecturas de memoria (`GetFixedField`, `ArrayGetIndex`, globals, upvalues), que se descartan ante la primera instrucción no pura |
| `fixed_fields` | **Scalar replacement de literales de objeto**: reenvía `GetProperty` sobre un `BuildObject` que no escapa al valor ya en SSA, y DCE elimina la asignación. *No* es el lowering a slot — eso ocurre en `hir/lower/expr.rs` con la info del checker |
| `licm` | Hoisting de invariantes: aritmética typed, `LoadGlobal` en loops sin efectos, y `GetFixedField`/`ArrayGetIndex` en loops donde toda instrucción es pura. **Literales no** — ver nota en `licm.rs` |
| `dce` | Eliminación de código muerto. La pureza es un **allow-list exhaustivo**: un `InstKind` nuevo rompe la compilación hasta clasificarlo, porque el deny-list anterior borraba getters con efectos |
| `cfg` | Simplificación y compactación del grafo de control |

`ssa/uses.rs` es el único lugar que sabe qué campos de un `InstKind` son
operandos. `replace_all_uses`, `verify::inst_uses` y `licm::operands` se
expresan sobre sus dos visitantes exhaustivos; antes eran tres copias
independientes y la tercera fallaba abierta.

### Costo medido

Añadir `cse` + `algebraic` + el LICM ampliado cuesta ~7% en `tests/main.vn`
(mediana 193.85 ms → 208.18 ms p50 e2e, 9 rondas alternadas de 30 corridas,
más lento en 9/9). Esa suite ejecuta cada test una vez: mide throughput de
compilación, no de ejecución. En un loop sobre campos de objeto el mismo
conjunto de passes rinde 1.67x (99.11 ms → 59.35 ms). El trade es
deliberado y hay que revisarlo con medición, no por intuición.

Inspección: `vn debug -p ssa <archivo>`. Traza del compilador: `VN_OPT_TRACE=1`.

---

## 4. Post-passes de bytecode (`varn-backend`)

`varn_backend::run_post_passes(&mut proto)` recorre además, recursivamente, cada
`PoolEntry::Function` anidado:

- `regalloc_post::optimize_function` — asignación de registros sobre el bytecode ya
  emitido, apoyada en `liveness`.

No existe ningún módulo `slot_kinds`. El `register_meta` del que depende el JIT
para saber qué registros puede mantener sin flush se deriva en `ssa/emit.rs`
(`derive_register_meta`, por registro pre-coalescing) y se re-mapea al
coalescer en `regalloc_post`. Cambiar la anchura de un slot en un lado y no
reflejarlo en el otro produce corrupción silenciosa en el código generado.

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
- Inmediatos: `AddImm`, `SubImm` — los emite `ssa::emit` (`plan_immediates`)
  cuando un operando de un `Add`/`Sub` typed es un `ConstInt` que entra en un
  `i8`. Si **todos** los usos de esa constante se pliegan, su `LoadInt`
  desaparece junto con el registro. Medido en el intérprete, loop de 30M
  iteraciones: ~893 ms con el inmediato contra ~1001 ms sin él (~11% por
  sitio). En el JIT es neutro — Cranelift ya funde `iconst` + `iadd`

Lo mismo para el acceso a propiedades: si el checker conoce la clase, el pass
`fixed_fields` emite `GetFixedField slot` / `SetFixedField slot` en lugar de
`GetProperty "nombre"`, y el JIT los baja a un único load/store con offset
constante.

### Top-level `let` privado → registros de `<module>`

`Scope::is_global()` es cierto en el frame del módulo, así que un `let` de nivel
superior compila a **slot global**: cada lectura son dos loads dependientes
(`ExecCtx` → puntero de `values` → slot) más los shifts de unbox, y cada escritura
un store. Como registro es solo un registro. `hir::module_locals` promueve los que
puede.

La promoción es sana **solo** si toda lectura ocurre en el frame del propio módulo.
Cuatro cosas la sacan de ahí, y cada una es requisito de corrección, no heurística:

* **Funciones top-level** se bajan con un `Scope::new()` fresco, así que
  `resolve_upvalue` devuelve `None` de inmediato y lo que referencien cae a global.
  Una variable promovida simplemente no se encontraría.
* **Closures, clases y enums** leen globals directamente en vez de capturarlos.
* **Exports**: otros módulos los leen por slot.
* **Miembros de namespace**: se declaran como globals y se releen para construir el
  objeto del namespace.

Las dos últimas se filtran en el sitio de declaración (`lower`, el único punto que
sabe que una escritura a global es una DECLARACIÓN y no una asignación). Las dos
primeras las responde el walker.

**El walker es exhaustivo a propósito.** `module_locals::walk_stmts` cubre cada
variante de `HirStmt` y `HirExpr` sin brazo `_`. Un contenedor añadido al HIR y
omitido ahí escondería un uso, y el pase promovería un global que un closure aún
lee — código mal, sin error de compilación. Con el match exhaustivo, añadir una
variante rompe el build. La primera versión recorría solo `HirClass::methods` y se
saltaba `getters`/`setters`/`ctor`/`static_methods`/`static_blocks`; lo atrapó
`tests/60-dce-purity.vn`. Cubierto ahora por `tests/68-module-locals.vn`.

Hay un tope de `MAX_PROMOTED` porque cada promoción añade un registro al frame del
módulo y `ssa::emit` falla pasados 255: un programa que hoy compila no puede dejar
de compilar por una optimización.

**Medido**, pareado en la misma ventana (top-level contra el mismo código envuelto
a mano en una función):

| caso | brecha antes | después |
|---|---|---|
| loop de 5M con acumulador y contador | 2.35× | **1.00×** |
| `bench_array_ops` | 2.14× | **~1.00×** |
| `bench_matrix` | 37 ms | **26 ms** |

En los tres el top level pasa a correr como el mismo código escrito dentro de una
función (`bench_matrix` cae justo sobre los 26.7 ms de la versión envuelta a mano).

Lo que **no** ayuda es hoistear la *carga* de un global invariante fuera de un loop:
el compilador ya la hace una sola vez antes del bucle, así que un LICM sobre
`LoadGlobalIdx` se implementó, midió cero y se descartó. El costo que P1 elimina es
el de las variables **mutables**, que el loop reescribe y por tanto nadie puede
hoistear: son load+store por iteración como global, y un registro como local.

Usar `benchmarks/compare.ps1` para volver a medir esto; ojo con tomar cifras con la
máquina cargada, que fue lo que en su momento hizo parecer que `bench_matrix` no se
movía.

### El recorrido de anotaciones tiene que ser completo

Nada de lo anterior sirve si el checker nunca visita la expresión. `annotate_expr`
(`varn-checker/src/checker_annotations.rs`) baja por el AST registrando lo que el
backend luego consume — kind numérico, intrínseco, op-id nativo, índice de array,
slot de campo fijo, tipo del resultado — y termina en un `_ => {}`.

Ese catch-all ya escondió el mismo bug tres veces: las interpolaciones de template,
los argumentos de constructor y los valores de object literal no se visitaban, así
que **toda** expresión dentro de `${...}`, de `new User(...)` o de `{ k: ... }`
perdía sus anotaciones. La aritmética ahí bajaba a `mod.dyn`/`add.dyn` — dispatch de
tipos en runtime — teniendo el checker los tipos a mano. Medido en
`benchmarks/bench_dto.vn`, cuyo loop construye 100k objetos con aritmética en los
argumentos: 31 ms → 25 ms al visitar los argumentos de `new`.

Un contenedor nuevo en `ExprKind` que no se añada a ese match no da error de
compilación ni falla ningún test: el único síntoma es código más lento. Al añadir
una variante que contenga expresiones, añadirla también aquí.

Hay **135 opcodes** (`crates/varn-core/src/opcode.rs`). No llevan prefijo `Op`.

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
