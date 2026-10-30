# ADR 0023: Contrato de la aritmética y de los errores nativos

## Estado
Implementado (2026-10-31).

## Contexto
Una auditoría de toda la aritmética (enteros, floats, bigint, decimal, rangos,
`Bytes`, cadenas, `std:math`, `std:time`) comparó el compilador, el intérprete y
el JIT contra un oráculo independiente. Los fallos que apareció no eran de
sintaxis: eran sitios donde un hecho numérico se calculaba dos veces de forma
distinta, o donde un límite se convertía en un valor plausible en lugar de un
error.

## Decisiones

### Una sola definición por hecho
- `varn_core::pow_int`, `pow_decimal`, `fmax`, `fmin`, `fsign`, `float_to_fixed`,
  `slice_bounds`, `clamp_index` son la única fuente. El intrínseco de la VM, los
  nativos y las funciones de Varn llaman a la misma.
- `RangeData` cuenta en `i128`; `len()` es `None` cuando no cabe en `int`.

### Valores que no caben son errores, nunca centinelas
- `int`: desbordamiento → `IntegerOverflow`; exponente negativo y cantidad de
  desplazamiento fuera de `0..=63` → `RangeError`; división por cero →
  `DivisionByZero`.
- `Float.parse` falla con `ParseError` (`tryParse` devuelve `null`);
  `Float.toInt` y `as int` rechazan NaN, infinitos y fuera de rango.
- `Bytes`: índice y valor de byte fuera de rango → `RangeError`.
- Tamaños acotados con `RangeError`: bigint 2^30 bits, cadena 2^29 bytes,
  `Bytes.alloc` 2^31 bytes, `Range.toArray` 2^28 elementos, dígitos decimales 2^22,
  `toFixed` 0..=100.

### Reglas numéricas
- NaN se propaga en `min`, `max`, `clamp`, `sign` y `Stats`; `-0 < +0`.
- `round` y `toFixed` de float redondean las mitades exactas alejándose de cero;
  `Decimal.round`/`toFixed` usan mitad-al-par. Es una diferencia deliberada por
  tipo, no un descuido.
- `/` y `%` truncan hacia cero; `rem` es el resto truncado y `mod` el resto
  euclídeo (nunca negativo).
- `~` solo existe para `int`; `bigint` y `decimal` no tienen operadores de bits.
- Los literales enteros solo se ensanchan a `float` cuando son exactos.

### `@throws(Clase, ...)` sustituye a `@fallible`
Un nativo declara las clases de error que puede lanzar. Los nombres se validan al
compilar contra `RuntimeErrorKind` y `StdErrorClass`; en ejecución un error de
clase no declarada (ni descendiente de una declarada) se convierte en un fallo de
contrato explícito. `Error` significa "cualquiera" y se usa en los nativos que
propagan errores de un callback.

## Consecuencias
- Cambios incompatibles aceptados (Ley 10): `Float.sign()` devuelve `float`;
  `Float.parse`, `toFixed`, `clamp`, `toInt` pueden lanzar; `Bytes.getByte` ya no
  responde `0` fuera de rango; `Duration/Instant/PlainDateTime.compare` devuelven
  -1, 0 o 1.
- Pendiente de decisión del dueño: los campos de `Duration` mezclan totales
  (`hours`, `seconds`, `days`) y componentes (`minutes`, `milliseconds`); los
  tests actuales fijan esa mezcla.
