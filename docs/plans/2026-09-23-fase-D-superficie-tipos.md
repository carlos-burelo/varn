# Fase D — Superficie del sistema de tipos

> Cada paso compila, deja `tests/main.vn` verde en JIT y `VARN_NO_JIT=1`
> (caché limpio, también con `VARN_STD=@embedded`), corpus negativo y goldens
> verdes, y es un commit. Spec §21–§31, §42, §45, §53, §104.

**Goal:** que la superficie de tipos del spec exista: `never` bottom, `Unit`,
literal types, exhaustividad sobre literales, `Record<K,V>` prohibido con
diagnóstico propio, `Map<K,V>` como colección real separada de las index
signatures, `Range<T>` genérico y perezoso, e intersecciones con conflicto.

## Situación medida (2026-09-23, `fcbd7ad6`)

| Probe | Resultado |
|---|---|
| `function g(): int { return fail() }` con `fail(): never` | VN3001 "returns 'never'" — `never` no es bottom entre primitivos |
| `let u = ()` | VN2007 `unit paren — should be handled by arrow parser` |
| `type M = "GET" \| "POST"` | VN2007 `literal types are not supported` |
| `let r: Record<str, int> = #{a: 1}` | VN3001 mismatch accidental, sin regla |
| `let m: Map<str,int> = o` (`o: {[k: str]: int}`) | aceptado — Map ≡ index signature |
| `let m: Map<str,int> = new Map()` | VN3001 `initialised with 'Map<dynamic>'` |
| `let m: Map<int,str> = …; m.set(1, "x")` | no compila: `map.vn` declara `Map<V>` con claves `str` |
| `let r: Range<int> = 0..10` | VN3001 `Range` no es genérico |
| `(0..10).step(3)` | devuelve `[0, 3, 6, 9]` (array), no un rango |
| `'a'..'e'` | `[object]` |
| `class ==`, `{} ==` referencial; `#{}`/`#[]` estructural | ✅ correcto, sin tests dedicados |
| `A & B` sobre interfaces | ✅ miembros de ambos; conflicto sin probar |

## Pasos

### D.1 `never` es bottom
`simple_types_compatible` acepta `never` como inferido para cualquier
declarado (hoy solo lo hace `types_compatible_impl` fuera de la vía rápida
de primitivos). Test en `tests/`: función `never` usada donde se espera
`int`, `str`, `T?`, clase.

### D.2 `Unit`
`()` es expresión y tipo. Representación: la tupla vacía (`#[]`), de modo que
igualdad estructural, layout e impresión salen del camino de tuplas; se
imprime `()`. El tipo se escribe `Unit` o `()` y es el tipo tupla vacía.
`void` sigue siendo solo retorno; usar el resultado de una
función `void` como valor es error (`VoidValueUsed`).

### D.3 Literal types
`TypeKind::Literal(TypeLiteral<N>)` con `Int(i64) | Str(N) | Bool(bool) |
Char(char)`. Parser: literales en posición de tipo (quita VN2007). Checker:
`"GET" <: str`; un literal de expresión satisface un destino literal igual;
un `str` no. Tipo aparente (`Type::apparent`) = primitivo base, usado por
inferencia de operadores, miembros y `emit::ty` (baja a su `BackendTy`
base). `let` sin anotación sigue infiriendo el primitivo base.

### D.4 Exhaustividad sobre literales y `T?`
`check/exhaustiveness.rs` trata una unión de literales como enum cerrado y
`T?` como `T | null`. `match` sobre `"GET" | "POST"` sin brazo `_` es
exhaustivo si cubre ambos; si falta uno, diagnóstico con el literal faltante.

### D.5 `Record<K,V>` prohibido
Diagnóstico dedicado `ForbiddenRecordGeneric` con sugerencia `Map<K,V>` o
`{ [key: K]: V }`, en resolución de tipos, antes de cualquier compat.

### D.6 `Map<K,V>` real
`map.vn` pasa a `Map<K, V>`; los natives de `map.rs` toman la clave como
valor (`MapKey(VmValue)` ya existe en runtime). Borrar las ramas de
`compat/mod.rs` que igualan `Map` con objetos/index signatures. `new Map()` y
`new Set()` toman sus argumentos del tipo esperado (`let`, parámetro,
retorno, campo). Cierra la deuda de Fase B (`Set<bigint>` en lugar de
`Map<bigint, …>` en el test).

### D.7 `Range<T>`
`range.vn` pasa a `Range<T>`; `0..5 : Range<int>`, `'a'..'e' : Range<char>`;
`step(n)` devuelve `Range<T>` perezoso (iterable), `toArray()` materializa.

### D.8 Object / Record / Class
Tests dedicados de igualdad (`==` referencial en Object y Class,
estructural profunda en Record y Tuple, incluidos anidados) y de
inmutabilidad de Record/Tuple.

### D.9 Intersecciones
Miembros = unión de miembros; un miembro con tipos incompatibles en ambos
lados es `never` (y un valor no puede satisfacerlo). Intersección de
primitivos distintos (`int & str`) es `never`.
