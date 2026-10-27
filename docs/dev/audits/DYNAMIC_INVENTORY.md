# Inventario de contratos dinámicos — 2026-10-26

Estado observado en fuentes y ejecución de `e89353bd`. Este inventario no
autoriza nuevas rutas dinámicas. Las referencias textuales cuentan productores,
consumidores y brazos de match; no equivalen a valores sin tipo en ejecución.

## Alcance reproducible

Descubrimiento inicial mediante el grafo del proyecto; su índice conservaba rutas
anteriores a la extracción de `varn-emit`. Las búsquedas siguientes usan `tgrep`
desde la raíz. Se contrastó su índice con `--no-index` para contratos y razones
backend; los conjuntos coincidieron.

```powershell
tgrep --no-index -n '\bdynamic\b' . -g 'std/**/*.vn' -g 'crates/varn-builtins/**/*.vn'
tgrep --no-index -n -o 'DynReason::[A-Za-z_]+' . -g 'crates/**/*.rs'
tgrep --no-index -n -o 'Type::Dynamic|LangPrimitive::Dynamic|CheckerTyId::DYNAMIC' . -g 'crates/**/*.rs'
```

| Universo medido | Referencias | Archivos | Detalle |
|---|---:|---:|---|
| Líneas con `dynamic` en contratos/std | 58 | 16 | [CSV](dynamic-contracts.csv) |
| Ocurrencias explícitas `DynReason::…` en Rust | 307 | 62 | [CSV](dynamic-backend-references.csv) |
| Ocurrencias de las tres formas semánticas buscadas | 158 | 78 | [CSV](dynamic-semantic-references.csv) |

La tercera búsqueda excluye abreviaturas importadas como `P::Dynamic`; no es un
censo de todos los productores semánticos. Los CSV conservan archivo, línea y
texto encontrado; sus posiciones corresponden al commit indicado.

## Contratos `.vn`

| Dominio | Archivos | Líneas | Uso observado |
|---|---:|---:|---|
| Presentación e IO | 2 | 9 | `print`, `debug`, consola, escritura variádica |
| Reflexión y metadata | 3 | 18 | objetivos heterogéneos, introspección, almacenamiento |
| Tareas y canales | 3 | 9 | callbacks, argumentos, defaults genéricos |
| JSON, CSV, TOML | 4 | 15 | entrada/salida de decoders y contenedores heterogéneos |
| SQLite | 1 | 2 | parámetros y filas |
| Tipos core | 2 | 2 | `flat`, restricción de `ReturnType` |
| Aserciones std | 1 | 3 | conversiones y comparación explícita |

Que un contrato exista no prueba autorización según Ley 17. Antes de migrarlo
hay que decidir su representación estática y migrar sus consumidores juntos.

## Razones backend

Las 18 variantes están en `crates/varn-tir/src/ty.rs`.

| Familia | Variantes | Qué conserva el contrato |
|---|---|---|
| Formas | `Shape`, `Interface`, `Record` | `ShapeId`, campos, tipos, opcionalidad y readonly |
| Contenedores y ejecución | `Awaitable`, `Yields`, `Range`, `TaskHandle` | `TyId` del contenido |
| Borrado de contratos | `Union`, `IndexSignature`, `Callable` | Razón sin miembros, índices o firma completa |
| Declaración o resolución incompleta | `Declared`, `Generic`, `Unrecorded` | Sin payload de tipo concreto |
| Fronteras y transporte | `HostBoundary`, `CoreMember`, `Spread`, `Boxed`, `Runtime` | Razón sin payload; requiere inspección del productor |

`Declared` también aparece como fallback de resolución en `varn-emit/src/ty/lower.rs`;
no distingue por sí solo una anotación explícita de un tipo perdido. Una
representación boxed tampoco demuestra por sí sola semántica dinámica.

Medición de una sola fixture:

```powershell
target/release/vn debug -p tir:check tests/260-json-decode-validates.vn
```

Resultado: 194 nodos, 78 estáticos, 5 por nombre, indicador estático 94%.
Dinámicos por razón: union 3; declared 1; function type 62; task 7;
object type 11; runtime 1; type parameter 12. Los cinco accesos por nombre
corresponden a object type. Las conversiones a SSA y proto informan OK.
Estas cifras no describen el conjunto del compilador.

## Agujeros reproducidos en VM y JIT

Los ejemplos siguientes aceptan valores que incumplen el tipo. Se ejecutaron
con `target/release/vn run <archivo>` y nuevamente con `VARN_NO_JIT=1`, usando
un caché aislado bajo `target/tmp/`. Ambas modalidades coincidieron.

### Unión sin miembros en el contrato backend

```typescript
import {Json} from "std:encoding";
type Choice = #{left:int} | #{right:str};
print(Json.parse<Choice>('{"neither":true}'));
const incoming:dynamic = {left:1};
const claimed:Choice = incoming;
print(claimed);
```

Imprime `{ neither: true }` y `{ left: 1 }`; el segundo valor sigue siendo un
objeto mutable. `varn-emit/src/ty/union.rs` produce `DynReason::Union` sin
miembros, y `varn-compiler/src/from_tir/describe.rs` lo describe como `Any`.

### Restricción literal borrada

```typescript
import {Json} from "std:encoding";
type Answer = "yes";
print(Json.parse<Answer>('"no"'));
```

Imprime `no`. En `varn-emit/src/ty/lower.rs`, `TypeKind::Literal` baja al
primitivo base: la igualdad con el literal no llega al descriptor.

### Índices: campos fijos y tipo de clave perdidos

```typescript
import {Json} from "std:encoding";
type Hybrid = {fixed:int, [key:str]:int};
type NumericIndex = {[key:int]:str};
print(Json.parse<Hybrid>('{"fixed":"wrong"}'));
print(Json.parse<NumericIndex>('{"x":"yes"}'));
```

Imprime ambos objetos. `lower_shape` borra las formas mixtas a
`IndexSignature`, descrito como `Any`; el caso de un único índice descarta
`key_ty` y solo conserva el tipo de valor en `BackendTy::Dict`.

### Firma de función no comprobada al convertir

```typescript
const candidate:dynamic = () => "wrong";
const claimed:()=>int = candidate;
print("function accepted");
try { print(claimed()+1); } catch(e) { print(e.message); }
```

La asignación pasa. Después de las correcciones de retornos boxed, consumir el
resultado lanza un TypeError capturable: `expected 'int', got 'str'`. Antes
provocaba un panic o interpretaba el payload como entero en caliente. La
protección del consumo está cerrada; comprobar la firma completa sigue pendiente.
`TypeKind::Fn` baja a `Callable`; incluso `BackendTy::Fn` se describe únicamente
como `TypeDesc::Callable` en `from_tir/describe.rs`.

### Intersección mixta pierde la obligación de ser record

```typescript
type View = #{left:int} & {readonly right:str};
const plain:View = {left:1,right:"s"};
print(plain);
```

Acepta e imprime un objeto mutable. La normalización de
`varn-sem/src/resolve/intersections.rs` conserva Record cuando todos los
constituyentes son records, pero mezcla esta forma como Object. No basta cambiar
la condición a «alguno es record»: propiedades mutables y opcionales requieren
resolver el meet semántico, incluidos los casos incompatibles.

## Controles y validación

`List<T> = T[]` rechaza `[1,"x"]` cuando se pide `List<int>`. La intersección de
dos records rechaza un escalar y decodifica una forma válida como record; sus
alias también conservan igualdad por valor. No se generalizan pérdidas a esas
formas a partir de un fallback sin reproducción.

El callback inválido repetido 2000 veces produce 2000 TypeErrors capturados y
permite continuar en ambas modalidades. La suite existente comprueba 4000
rechazos de retornos con tags incorrectos. `verify.ps1 -Fast` pasó tras el último
cambio de código: 3959 comprobaciones, cero fallos en los cuatro cuadrantes y
en la comprobación adicional del caché JIT. También pasaron formato y clippy.

## Orden de reparación

Primero conservar los contratos semánticos en TIR/descriptor: literales,
miembros de uniones, firmas e índices. El runtime debe consumir ese contrato,
sin reconstruirlo por nombres ni añadir rutas paralelas. El meet mixto se
corrige en semántica antes de bajar su resultado. Después se migran las APIs
heterogéneas del inventario y se completan las auditorías aún abiertas.
