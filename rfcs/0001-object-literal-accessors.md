# RFC 0001: accesores y `this` en literales de objeto

- Estado: draft
- Fecha: 2026-10-24

## Resumen

Un literal de objeto que declara `get`/`set`, o métodos, se compila como instancia de una
clase anónima sintetizada en ese sitio. Los literales de solo datos no cambian.

## Motivación

El parser acepta `{ get x() { … } }` y el checker lo rechaza con `VN5001`. Además, hoy un
método de literal es un campo con una clausura, sin receptor:

```vn
const o = { n: 3, twice() { return this.n * 2 } }
o.twice()   // error: cannot read property 'n' of null
```

Ambos casos tienen la misma causa: el literal de objeto no tiene dónde vivir código con
receptor. Las clases sí.

## Estado actual (medido)

- `infer_object_type` ignora `Getter`/`Setter` al construir el tipo; `lower_object` los descarta.
- Un método de literal baja a `TirObjectEntry::Field` con una clausura de tipo `Dynamic(Callable)`.
- Una clase con `get x()` ya es asignable a una interfaz `{ x: int }` y se lee bien a través de ella
  (comprobado: `readX(new C())` → 8, en los dos tiers).
- `ExprKind::ClassExpr` se infiere como `Type::Dynamic` (`infer_impl.rs`); `new class {…}()` falla en runtime.

## Diseño

1. **Un solo mecanismo para código con receptor: la clase.** Un literal con al menos un
   `Method`, `Getter` o `Setter` se sintetiza como clase anónima con los campos `Property` como
   campos de instancia y el resto como miembros. `this` es el receptor.
2. **El tipo del literal es el tipo nominal sintetizado.** Sigue siendo asignable a tipos
   estructurales (`{ x: int }`) por la regla que ya existe para clases.
3. **Spread.** `{ ...o }` con `o` sintetizado copia el valor actual de cada propiedad legible
   (campos y getters), nunca el accesor. El resultado es un literal de datos.
4. **`with`.** `o with { … }` sobre un objeto sintetizado produce un literal de datos.
5. **Setter sin getter** es de solo escritura; getter sin setter es de solo lectura. Asignar a uno de
   solo lectura es error del checker, igual que en clases.
6. **Prerrequisito (Ley 14).** `ClassExpr` debe tiparse como el tipo de su clase, no `Dynamic`. El
   literal sintetizado entra por ese mismo camino.

## Ganancia y verificación

- Corrección: `this` en métodos de literales deja de ser `null`; `VN5001` desaparece.
- Tipado estático: el literal con comportamiento deja de bajar a `Dynamic(Callable)`; los accesos
  son dispatch estático como en clases.
- Verificación: casos nuevos en `tests/` (getter, setter, `this`, spread, `with`, paso a interfaz,
  asignación a solo lectura) en los 4 cuadrantes; `vn debug -p bytecode` sin `GetProperty` genérico
  sobre el literal.

## Qué se borra

- La rama `Method` de `lower_object` (campo con clausura sin receptor).
- El diagnóstico `VN5001` y los brazos vacíos de `Getter`/`Setter` en `infer_object_type`, `lower_object`
  y `lower_with`.
- `Type::Dynamic` como resultado de `ClassExpr`.

## Incompatibilidad

Un método de literal pasa a tener receptor propio. Extraerlo (`const f = o.twice`) deja de ser una
clausura libre. Los métodos que no usan `this` no cambian de comportamiento.

## Alternativas

- **Quitar `get`/`set` del parser y del AST.** Más pequeño, pero deja el bug de `this` y no
  aporta ganancia de tipado.
- **Accesores como campos con clausura (dispatch por nombre en runtime).** Rechazada: lookup
  dinámico por nombre, Ley 17.

## Pasos de migración

1. Tipar `ClassExpr` como su clase y arreglar `new class {…}()`.
2. Sintetizar la clase para literales con miembros; borrar la rama `Method` de `lower_object`.
3. Spread y `with` sobre sintetizados.
4. Quitar `VN5001`; documentar en `docs/lang/` y `grammar.md`.

## Preguntas abiertas

- ¿Se admite `super` o `static` en el literal? Propuesta: no.
- ¿Un literal con accesores es igual (`==`) a otro con los mismos valores? Propuesta: no; igualdad
  nominal por sitio, como instancias de clase.
