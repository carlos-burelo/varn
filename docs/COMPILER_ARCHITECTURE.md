# Arquitectura del Compilador (varn-compiler)

`varn-compiler` transforma el TypedAST de `varn-checker` en `FunctionProto` / bytecode ejecutable por `varn-vm`.

## 1. Pipeline

```
TypedAST + SemanticDB
        │
        ▼
  varn-compiler
        │
    ┌───┴────────────────┐
    │                    │
Direct Emission       IR Lowering
(ruta principal)      (varn-ir, SSA)
    │                    │
    └───────┬────────────┘
            │
        FunctionProto
        (bytecode, constant pool)
```

### Direct Emission
Iteración sobre AST con `compile_stmt`/`compile_expr`. Single-pass modificado — la info de tipos de `varn-checker` se consulta inline. Produce el `FunctionProto` principal.

### IR Lowering (varn-ir)
Pipeline paralelo para AOT. Aplana AST a SSA (Static Single Assignment) en `IrBlock`s conectados por `IrTerminator`. `IrEmitter` genera bytecode final. Permite DCE, constant folding futuro.

---

## 2. Slots y Registros

La VM es register-based. El compilador asigna slots estáticos en compile-time:

- `scope.declare_local("x")` → slot numérico (ej. slot 3). El string `"x"` no existe en bytecode.
- `OpSetLocal 3` / `OpGetLocal 3` — acceso O(1) por offset.
- Al cerrar un bloque `{ }`, el compilador elimina los slots del bloque.

### Upvalues
Si una función anidada referencia una variable del padre, se promueve a `Upvalue`. Al cerrar el frame padre, `OpCloseUpvalue` mueve el valor del registro al heap.

---

## 3. `using` y Disposables

`using db = connect()` registra `db` en un vector `disposables` atado a la profundidad del bloque actual.

Cualquier escape prematuro (`return`, `break`, `continue`) invoca `emit_dispose_cleanup()`, que inyecta el bytecode de `dispose()` antes del salto. Garantía determinista sin cost en runtime.

---

## 4. Clases y Optimizaciones de Acceso

### Fixed Field Slots
Si el compilador sabe que `this.message` es el slot 0 de la clase (por info del checker), emite `OpSetFixedField 0` en vez de `OpSetProperty("message")`. Elimina el hash lookup en runtime.

### Virtual Table
`instancia.metodo()` con clase conocida → `OpInvokeVirtual idx`. Dispatch directo por índice de vtable.

---

## 5. Control de Flujo y Back-Patching

Saltos se emiten hacia índice dummy 0, guardados en listas de pendientes, y "parcheados" al conocer la dirección final:
- `break` → guarda en `break_patches`, parchea al final del loop.
- `continue` → guarda en `continue_patches`, parchea al inicio del loop.
- `if/else` → parchea el salto del `if` al `else`, y el salto del `else` al final.

Antes de cada salto: `OpPop` para los locals del bloque actual (desalinear la VM).

---

## 6. Try/Finally Inlining

El bloque `finally` se guarda en `try_stack`. Si aparece `return`/`break` dentro de `try` o `catch`, el compilador **inlinea** el bytecode de `finally` directamente antes del salto — sin desenrollamiento en runtime.

---

## 7. Desugaring

### Destructuring
`const { a, b, ...rest } = obj`:
1. Variable temporal secreta para `obj`.
2. `GetProperty "a"` → slot para `a`.
3. `OpObjectRest` para `rest` — volcado eficiente de claves no usadas.

### Enums
`Option::Some(val)` → closure constructor que emite `OpMakeEnumVariant` con tag y payload.

### Namespaces
`namespace Tools { export function doWork() {} }` → `OpBuildObject` con todas las exportaciones.

### Extensions
`"hola".trim()` donde `trim` es extension de `str` → el compilador sustituye por llamada directa a la función de extensión. Cero hash lookup de propiedad.

---

## 8. Decoradores

`@deco function foo() {}`:
1. Compila `foo` como closure.
2. Llama `deco(foo, ctx)` donde `ctx = { name, kind, isStatic }`.
3. Si `deco` retorna valor, reemplaza `foo`. Si retorna null, preserva el original.

---

## 9. Peephole Optimizations

- `OpPushNull` + `OpPop` → eliminados.
- `OpSetLocal` + `OpPop` → fusionados en `OpSetLocalDrop`.
- Operadores sobre tipos `int` conocidos: `OpAdd` → `OpAddI32`. Para `float`: `OpAddF64`. Bypasean la comprobación de tipo NaN en runtime.

---

## 10. Constant Pool

Literales (`str`, `float`, `decimal`, arrays constantes) se deduplicados en el `constant_pool` del `FunctionProto`. Las instrucciones usan índices. Las funciones anidadas se almacenan como `PoolEntry::Function`.
