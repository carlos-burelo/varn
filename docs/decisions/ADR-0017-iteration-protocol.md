# ADR 0017: Protocolo de iteración estático (`iter()` + `next(): Option<T>`)

## Estado
Aceptada (2026-09-24), con el usuario. Spec §33, §44, §46, §49.

## Contexto
Hoy `for-of` sobre algo que no es array/rango baja a `IterInit` + llamadas
por nombre, y el protocolo es el de JS: una clave-símbolo
`[Symbol.iterator]` resuelta en runtime y un `next()` que devuelve un objeto
`{ value, done }` por paso (el runtime reutiliza un `__res` mutándolo para
no alocar). Las clases de usuario ni siquiera pueden declarar
`[Symbol.iterator]`. Eso contradice D2 (`Symbol` fuera del núcleo), hace de
`value` un `dynamic` y reintroduce dispatch por nombre (§34).

## Decisión
```varn
interface Iterable<T> { iter(): Iterator<T> }
interface Iterator<T> { next(): Option<T> }
```
- Métodos con nombre normal, satisfechos estructuralmente (§32); sin
  símbolos ni claves calculadas.
- `for (const x of e)` se resuelve por el tipo estático de `e`: arrays,
  rangos y mapas con bucles especializados (§46); un tipo de usuario por slot
  de vtable (`iter()`, luego `next()` por paso), con `x: T`.
- `Generator<T>` implementa `Iterator<T>`; `{ value, done }` sale de la
  superficie del lenguaje.
- `AsyncIterator<T> { next(): Future<Option<T>> }` se fija en la Fase I.

## Precondición
`Option<T>` no puede costar un objeto heap por paso: requiere el layout de
enums con nicho (Fase F): `Option<referencia>` = nicho `null`,
`Option<escalar>` = (valor, tag) en registro. El protocolo se implementa
después de ese paso.

## Alternativas descartadas
- `[Symbol.iterator]` + `{ value, done }` (JS): dispatch por nombre en
  runtime, objeto por paso, `value` dinámico.
- `hasNext()` + `next(): T`: dos llamadas por elemento y lookahead obligado en
  generadores.
- `next(): T?` con `null` como fin: ambiguo para `Iterator<int?>`.
