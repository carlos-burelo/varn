# RFC 0001: accesores y `this` en literales de objeto

- Estado: accepted (alternativa: rechazar accesores)
- Fecha: 2026-10-24

## Resumen

Los literales de objeto no tienen accesores (`get`/`set`) ni receptor (`this`). El comportamiento con
receptor se declara en una clase. El parser rechaza `get x() {}` y `set x(v) {}` en un literal, y el
checker rechaza `this` fuera de una clase, enum o extensión.

## Motivación

Antes de este RFC el parser aceptaba accesores en literales y el checker los rechazaba con `VN5001`; un
método de literal que usaba `this` compilaba y fallaba en runtime con «cannot read property … of null»:

```vn
const o = { n: 3, twice() { return this.n * 2 } }
o.twice()   // runtime: cannot read property 'n' of null
```

`this` tenía tipo `Dynamic` fuera de una clase (`infer_impl.rs`). Un valor `dynamic` que nadie pidió y que
revienta al ejecutarse es el síntoma de la Ley 17.

## Análisis con la Ley 17

Un literal es una forma sin tipo nominal. Soportar accesores obliga a elegir entre:

1. **Ranura de accesor en el objeto.** El tipo `{ x: int }` no distingue dato de accesor, así que cada
   lectura comprobaría la ranura en runtime y tocaría VM e IC del JIT. Es dispatch dinámico por forma:
   sin solución estática, requeriría autorización explícita.
2. **Sintetizar una clase anónima.** Las clases de Varn son tipos únicos del módulo que no capturan los
   locales de la función (`check_local_class_capture`); los métodos de literal sí capturan, así que se
   rompería código existente, o habría que añadir conversión de clausuras y celdas compartidas. Además
   `ClassExpr` hoy no es de primera clase (nombre `<anon>`, tipo `Dynamic`, se recoge solo en
   `const K = class {…}`).

Que JS/TS los tenga no es argumento. La ganancia de un accesor en un literal es nula frente a una clase,
que ya da getters, setters, `this` y dispatch estático, y es asignable a interfaces estructurales
(verificado: `readX(new C())` con `get x()` lee bien a través de `{ x: int }`).

## Decisión

- Se **borran** `ObjectProp::Getter` y `ObjectProp::Setter` del AST y todos los brazos que los llevaban
  (binder, sem, checker, emit, LSP, debug).
- `get`/`set` siguen siendo nombres de propiedad válidos: `{ get: 1 }`, `{ set() {} }`, `{ get }`.
- `get x() {}` / `set x(v) {}` en un literal es un error de sintaxis: «declare a class».
- `this` fuera de una clase, enum o extensión es `VN4115` (`ThisOutsideInstance`). Dentro de un método de
  literal también, salvo que el literal esté escrito dentro de una clase (entonces `this` es el de la clase).
- Los métodos de literal que no usan `this` no cambian.

## Ganancia y verificación

- Corrección: desaparece el fallo de runtime por `this` nulo; el error ocurre al compilar.
- Simplicidad: dos variantes de AST y unos 15 brazos menos; ninguna ruta dinámica nueva.
- Verificación: `tests/23-objects.vn` (`get`/`set` como nombres), suite completa 3804 pasados, 0 fallidos.

## Qué se borró

`ObjectProp::Getter`, `ObjectProp::Setter`, la rama de `VN5001` en `check_object_props` y la
re-exportación `parse_single_param` del parser.

## Incompatibilidad

Código con `this` en un método de literal fuera de una clase dejó de compilar. No había casos en la std ni
en `tests/`; en runtime ya fallaba.

## Si algún día se quieren accesores

Requiere antes (Ley 14) dar a `ClassExpr` identidad única por sitio y tipo nominal, y decidir cómo capturan
las clases locales (conversión de clausuras). Es un RFC nuevo, no una extensión de este.
