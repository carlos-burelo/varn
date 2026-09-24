# Fase E — Capacidades y operadores

> Cada paso compila, deja `tests/main.vn` verde en JIT y `VARN_NO_JIT=1`
> (caché limpio, también con `VARN_STD=@embedded`), corpus negativo y goldens
> verdes, y es un commit. Spec §25, §32–§34, §56.

**Goal:** las operaciones genéricas se describen con interfaces estándar
(estructurales, §32) y los operadores sobre tipos de usuario se resuelven por
capacidad, en compilación, sin dispatch por nombre.

## Situación medida (2026-09-24, `b91ca218`)

- No existe ninguna capacidad estándar; el prelude (`intrinsics.vn`) sólo
  declara `Iterator`, `AsyncIterator`, `TaskHandle`, `Disposable`,
  `AsyncDisposable`.
- No hay sobrecarga de operadores: `a + b` con `a: Vector` es VN3010.
- Las reglas de operadores primitivos ya son datos en `varn_core::numeric`
  (`binary_operand_kind`); `binary_ops.rs` las consulta junto con reglas de
  `str`.
- `std/time/duration.vn` ya tiene la forma de `Comparable`/`Equatable`
  (`compare`, `equals`, `add`).
- Claves de `Map`/`Set`: se canonizan por valor para primitivos, `bigint`,
  `decimal`, `char` y `str`; una instancia de clase es clave por identidad.

## Pasos

### E.1 Capacidades estándar en el prelude
`intrinsics.vn` declara, como interfaces estructurales:
`Equatable<T> { equals(other: T): bool }`, `Comparable<T> { compare(other: T): int }`,
`Hashable { hash(): int }`, `Cloneable<T> { clone(): T }`,
`Display { toString(): str }`, `Debug { debug(): str }`,
`Iterable<T>`, `Indexable<K, V> { get(key: K): V? }`,
`Add<T, R> { add(other: T): R }`, `Sub`, `Mul`, `Div` (`sub`/`mul`/`div`),
`Neg<R> { neg(): R }`. La tabla operador → método vive en `varn-core`
(`operator_capability`), una sola vez. Test: una clase de usuario satisface
cada una sin `implements`.

### E.2 Operadores por capacidad
Si el operando izquierdo no es primitivo y su tipo tiene el método de la
capacidad, el checker registra el operador como llamada (`AstId → método`,
junto a las llamadas de extensión en `DesugaredCalls`) y tipa el resultado con
el retorno del método:

| operador | método | resultado |
|---|---|---|
| `+ - * /` | `add sub mul div` | retorno del método |
| unario `-` | `neg` | retorno del método |
| `< <= > >=` | `compare` | `compare(b) ⋚ 0` |
| `== !=` | `equals` | `equals(b)` / `!equals(b)` |

El emit baja la llamada por el slot de vtable de la clase (sin búsqueda por
nombre). `===` sigue siendo identidad. Primitivos: sin cambio (tabla de
`varn_core::numeric`).

### E.3 Claves `Hashable`/`Equatable` en `Map`/`Set`
Una instancia cuya clase declara `hash()` y `equals()` es clave por valor:
el runtime canoniza la clave a un representante (primer objeto visto con igual
`hash` y `equals` verdadero), como ya hace con `str` y `bigint`.
