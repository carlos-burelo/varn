# Auditoría del diseño de UI y diseño definitivo

Alcance: `std/ui/**`, `std/signals`, `crates/varn-ui`, `crates/varn-builtins/src/modules/runtime/ui`, `crates/varn-text`.
Evidencia: código (Ley 12). Fecha: 2026-10-23.

Medidas tomadas del código: 11 clases con `override measure`; 60 métodos de protocolo entre `measure`, `paint`, `computeLayout` y `layout`; unas 2570 líneas en `std/ui`; unas 1200 en `varn-ui`; unas 700 en el runtime de la VM.

## 1. Hallazgos

| # | Hallazgo | Evidencia | Severidad |
|---|---|---|---|
| H1 | Dos protocolos de layout conviven: `measure/paint` y `layout/computeLayout`. La aritmética de flex está escrita dos veces. | `flex.vn` (`measure` y `computeLayout`) | Alta |
| H2 | Los componentes se ejecutan al construir el árbol. La identidad del estado sale de un contador por orden de llamada. | `state.vn` (`calls`, `scopeFor`), `component.vn` | Alta |
| H3 | El estado de un componente vive en un scope global indexado por (definición, ordinal). Dos instancias en padres distintos comparten el espacio de ordinales. | `state.vn` `scopeFor` | Alta |
| H4 | El registro de controles está duplicado en 6 arrays paralelos (`controlIds/Fns/Overlay` y `textIds/Fns/Overlay`), con la misma regla de gating modal copiada. | `frame.vn` | Media |
| H5 | El modal se decide por banderas globales (`overlayPass`, `modal`), no por el orden del árbol. | `frame.vn`, `render.vn` | Media |
| H6 | La entrada existe en tres representaciones: `VM Event`, `RealEvent` y `egui::Event`. La traducción se hace dos veces. | `ui.rs` (`to_real`), `input.rs` | Media |
| H7 | Los comandos de dibujo son estructuras de la VM que se formatean como strings para tests y se traducen a `Run`. Tres capas para lo mismo. | `draw.rs` (`format`), `paint.rs` | Media |
| H8 | Dos contextos de fuentes de egui: uno global para medir (`varn-text`) y uno por ventana para pintar. Pueden divergir. | `varn-text/src/lib.rs`, `canvas.rs` | Alta (correctitud) |
| H9 | La clase de ventana se re-deriva en layout y en paint con anchos que pueden diferir. | `responsive.vn`, `flex.vn` | Media |
| H10 | El scroll tiene dos fuentes de verdad (egui y la señal del usuario) con latencia de un frame. | `scroll.vn`, `paint.rs` (`paint_scroll`) | Media |
| H11 | El texto de un campo tiene dos fuentes (`TextEdit.text` y el buffer de egui), sincronizadas por valor en cada frame. Frágil con selección e IME. | `controls.vn` (`paintText`), `paint.rs` | Media |
| H12 | Los tests dependen de formato de strings (`"rect 0 14 ..."`), de conteos de `present` y de `fr.clicked`. Cualquier cambio interno rompe muchos tests. | tests 232, 241, 250, 252 | Media |
| H13 | Ids de control: en vn son `hashKey(path)` más un contador (módulo 2^31); en egui son `c.id`. Hay posibilidad de colisión y dos espacios de ids. | `base.vn` (`hashKey`), `paint.rs` | Media |
| H14 | API inconsistente: `Checkbox(value, onChange)` y `Switch(value, onChange)` con `onToggle` interno; `Field(edit)` frente a `Input(bind)`; `Text(value)` frente a `Button(text, onPress)`; `win.present()` devuelve int y `present(win, fr)` no. | `toggles.vn`, `controls.vn`, `window.vn`, `render.vn` | Media |
| H15 | `UiWindow.open` es headless por defecto y `run` abre ventana por defecto. Dos reglas para lo mismo. | `window.vn`, `render.vn` | Baja |
| H16 | `fr.clicked` ya no lo usa `std:ui` pero sigue expuesto, y lo usan tests. | `frame.vn`, test 215 | Baja |
| H17 | Nombres: `std:ui/text` apoya en `runtime:text`, y `varn-text` ya no tiene nada de Parley. | `std/ui/text.vn`, `varn-text` | Baja |

## 2. Causa raíz

H1, H2, H3, H5 y H9 vienen de una sola decisión: los componentes y los controles se describen mientras se construye el árbol, pero se identifican y se miden mientras se pinta. Como la descripción no tiene un lugar en el árbol, la identidad hay que inventarla con contadores, y cada fase recalcula lo que la otra ya sabía.

H4, H6 y H7 vienen de no tener un modelo de datos único para entrada, comandos y registros de control.

H8 y H11 son problemas de frontera: dos motores (medida y pintura) y dos fuentes de verdad para el texto.

## 3. Diseño definitivo

### 3.1 Árbol retenido con montaje

- `@component` construye un nodo de descripción sin ejecutar el cuerpo. Los contenedores reciben descripciones.
- El reconciliador monta cada descripción en una posición (ruta de padres más la clave de `Each`). Solo entonces se ejecuta el cuerpo del componente.
- El estado creado en el cuerpo (`signal`, `computed`, `effect`) pertenece al nodo montado y se libera al desmontarlo. No hay `scopeFor`, ni `calls`, ni flag `state:`. Todo nodo tiene dueño; uno sin señales no cuesta nada más.
- Identidad = ruta montada. Sin contadores ni id de sitio del compilador. El usuario solo escribe claves en `Each`.

### 3.2 Un solo protocolo de layout

- Cada nodo implementa `layout(c: Constraints): Placement`. Nada más mide.
- `paint` recorre el árbol de `Placement` ya calculado y no vuelve a medir.
- La clase de ventana viaja en `Constraints` (campo `windowClass`) y no se re-deriva. `Responsive` y `Adaptive` leen ese campo.
- Se borran `measure`, `fit` y `computeLayout` como nombres separados.

### 3.3 Un solo modelo de entrada

- Un único tipo `UiEvent` en Rust, usado por la VM y por egui. Sin `to_real` ni traducciones paralelas.
- Un solo canal de retorno por nodo: `node.on(kind, handler)` para clics, cambios, foco, envío y valor. Se acaban los arrays paralelos.

### 3.4 Modal y capas por árbol

- Los overlays son hijos de una capa superior del árbol montado. El hit-test recorre de arriba abajo y el primer nodo que consume el evento lo consume. El modal es un nodo que absorbe los clics debajo de él, no una bandera global.

### 3.5 Texto: un motor, una fuente

- Un único `TextSystem` por proceso, con un solo `Fonts` de egui, usado para medir y para pintar. Se borra la duplicación de `varn-text`.
- El texto editable tiene una sola fuente de verdad: el buffer de egui mientras el campo tiene foco. El valor del modelo se escribe al perder foco o en un `change` explícito. Así se evita la sincronización por valor en cada frame.

### 3.6 Scroll

- egui es la fuente de verdad del offset. La señal del usuario es un espejo que se actualiza después de presentar. La latencia de un frame queda documentada como regla.

### 3.7 Comandos

- La VM produce una lista estructurada (un enum de Rust, no strings). Los tests comprueban la estructura con accesores explícitos (`layoutOf(node)`, `rectsOf(node)`), no con el formato de texto.

### 3.8 API uniforme

Convenciones:
- El valor principal va primero: `Text(value)`, `Button(label, onPress)`, `Checkbox(value, onChange)`, `Switch(value, onChange)`.
- Los callbacks se llaman `onX` (`onChange`, `onPress`, `onSubmit`, `onActivate`).
- Un solo widget de campo: `Field(binding, multiline: bool = false)`, donde `binding` es `TextEdit` o `Signal<str>`. Sustituye a `Field`, `Input` y `TextArea`.
- `present(win)` como función única; se elimina `win.present()`.
- Una sola regla de ventana: `UiWindow.open(title, w, h)` abre ventana, y los tests pasan `headless: true` explícitamente.

### 3.9 Lo que se borra

`fr.clicked`, `nextSlot`, `hashKey`, `scopeFor`, `newComponentId`, `calls`, `touched`, los arrays paralelos de `frame.vn`, `to_real`, `DrawCmd::format` como contrato de tests, `varn-text`, `measure`, `fit`, `computeLayout`, el flag `state:`, `Input`, `TextArea` y `win.present()`.

## 4. Coste y orden

Es un cambio grande. Conviene hacerlo en este orden, con suite verde y commit en cada etapa:

1. Accesores estructurados para tests (H12) y `UiEvent` único (H6). Sin cambio de comportamiento.
2. Un solo `TextSystem` (H8). Cambian las medidas: revisar tests de texto.
3. Árbol retenido y montaje de componentes (H2, H3). Es el cambio de modelo: el cuerpo de un componente deja de ejecutarse al construir.
4. Un solo protocolo de layout y clase de ventana en `Constraints` (H1, H9). Reemplaza las etapas 2 y 3 previas del plan v2.
5. Registro único de controles y modal por árbol (H4, H5).
6. Renombres de API (H14 a H17) con los tests del repo actualizados en el mismo commit. No hay período de deprecación externo.

## 5. Decisiones

1. Montaje lazy de componentes (3.1): **sí.** Resuelve H2 y H3 de raíz. Elimina `scopeFor`, `calls` y el flag `state:` sin cambiar el código del usuario. Afecta a `Responsive`, a los tests que llaman componentes directamente y al `run` loop.
2. Renombre de API (3.8): **sí**, incluido el `Field` unificado. Se hace en el mismo commit que el montaje para no cambiar la API dos veces. Sin período de deprecación, porque no hay usuarios externos.
3. `UiWindow.open`: **ventana por defecto**, igual que `run`. Los tests pasan `headless: true` explícitamente, en un solo commit.

Orden: (1) registro estructurado para tests y `UiEvent` único, sin cambio de comportamiento; (2) montaje lazy junto con los renombres de la API y la regla de ventana por defecto.
