
# Varn Native UI Specification
Version: 1.0-draft
Module: `std:ui`

> **Estado de implementación (2026-10-17, verificado en `std/ui/`).**
> Hecho, en Varn sobre `runtime:ui`: señales `signal`, `computed`, `effect`,
> `batch`, `untracked` (`signal.vn`); decoradores `component` y `memo`
> (`component.vn`); `VNode` y el catálogo de primitivas de §8–§10 más
> `container`, `animated`, `when`, `each` (`catalog.vn`); estilos y temas (`style.vn`).
> Puerta: `tests/220-ui-declarative.vn`.
> Pendiente: el decorador `pure` (§12) y la UI IR y su pipeline de compilación
> (§16–§17); hoy el árbol se construye y se resuelve en tiempo de ejecución.

## 1. Objetivo

Definir una API declarativa para construir interfaces gráficas nativas en Varn, con composición funcional, estado reactivo, argumentos nombrados, decoradores y renderizado incremental.

La API debe integrarse con el sistema de tipos, AST, IR, compilador y runtime existentes de Varn.

Principios:

- Sintaxis ordinaria de Varn, sin JSX ni HTML como lenguaje de composición.
- Componentes funcionales con `@component`.
- Signals explícitas mediante funciones.
- Argumentos nombrados y tipados.
- Árbol visual independiente del backend de renderizado.
- Actualizaciones granulares sin reconstrucción indiscriminada de componentes.
- Layout declarativo y tipado.
- Renderizado nativo con aceleración GPU cuando corresponda.
- Separación entre API pública, UI IR, runtime y backend de plataforma.
- Sin dependencia de React, Flutter ni un runtime JavaScript.
- Sin introducir una segunda sintaxis de lenguaje innecesaria.

Los ejemplos definen la API objetivo. Los tipos y primitivas nativas son contratos conceptuales hasta que se implementen en Varn.

## 2. Módulo público

El módulo principal es `std:ui`.

```ts
import { ui, signal, computed, effect } from "std:ui"
```

Las primitivas visuales se exponen bajo `ui` para evitar colisiones con funciones y tipos de aplicación.

Las funciones de estado se exportan directamente para que su semántica sea independiente de la composición visual.

## 3. Componentes

Todo componente visual declarado por el usuario debe utilizar `@component`.

```ts
import { ui } from "std:ui"

@component
function Counter() {
    return ui.column(
        children: [
            ui.text("Counter")
        ]
    )
}
```

Reglas:

- Un componente es una función tipada que produce una descripción visual.
- Los componentes pueden invocar otros componentes.
- Los componentes reciben argumentos posicionales o nombrados según su firma.
- El compilador identifica las declaraciones decoradas como unidades visuales.
- El decorador no debe introducir una llamada indirecta obligatoria por actualización.
- Las primitivas internas de `std:ui` no requieren decoradores escritos por el usuario.
- Las funciones auxiliares y de negocio no requieren `@component`.
- Los componentes no deben acceder directamente al backend gráfico.

## 4. Argumentos nombrados

Las llamadas a componentes y primitivas admiten argumentos nombrados.

```ts
ui.column(
    gap: 12,
    padding: 16,
    children: [
        ui.text("Dashboard")
    ]
)
```

Reglas:

- Los nombres deben coincidir con la firma declarada.
- Los argumentos desconocidos producen errores de compilación.
- Los argumentos obligatorios deben proporcionarse.
- Los argumentos opcionales pueden tener valores predeterminados.
- El orden de los argumentos nombrados no altera la semántica.
- Los argumentos posicionales solo se permiten en las posiciones definidas por la firma.
- No se permiten argumentos duplicados.
- Los tipos se verifican estáticamente.
- Los argumentos se normalizan durante la compilación a la representación interna de propiedades.

La API no debe depender de objetos de configuración escritos manualmente para simular argumentos nombrados.

## 5. Signals

El estado reactivo se declara mediante funciones explícitas.

### 5.1 signal

```ts
const count = signal(0)

count()
count(10)
```

Contrato:

- `signal<T>(initialValue: T)` crea una celda reactiva.
- Invocar `count()` sin argumentos lee el valor actual.
- Invocar `count(value)` escribe un nuevo valor.
- La lectura dentro de una evaluación reactiva registra una dependencia.
- La escritura invalida los consumidores dependientes.
- El tipo de la señal permanece estático.
- El valor inicial determina el tipo cuando no se proporciona un genérico explícito.
- La igualdad determina si una escritura produce una notificación.
- Las escrituras deben respetar el orden de observación definido por el runtime.

La forma de invocación es conceptual; el lenguaje debe admitir una API tipada que distinga de forma inequívoca lectura y escritura.

### 5.2 computed

```ts
const count = signal(0)

const doubled = computed(() => count() * 2)

doubled()
```

Contrato:

- `computed<T>(derive)` crea un valor derivado.
- Las dependencias se detectan durante su evaluación.
- El resultado se memoriza hasta que una dependencia cambie.
- Las lecturas no deben ejecutar repetidamente el cálculo si sus dependencias permanecen válidas.
- Las funciones de cálculo deben ser puras.
- Las dependencias se actualizan dinámicamente cuando cambia el camino de evaluación.

### 5.3 effect

```ts
const count = signal(0)

const dispose = effect(() => {
    console.log(count())
})

count(1)
dispose()
```

Contrato:

- `effect(fn)` ejecuta una función reactiva con efectos observables.
- Registra las señales leídas durante su ejecución.
- Se vuelve a ejecutar cuando sus dependencias se invalidan.
- Devuelve una función de limpieza.
- No debe ejecutarse como parte de un cálculo puro de renderizado.
- Los efectos asociados a componentes se limpian al destruir el ámbito propietario.
- El runtime define la planificación y el orden de ejecución.

### 5.4 batch

```ts
const first = signal(0)
const second = signal(0)

batch(() => {
    first(1)
    second(2)
})
```

`batch` agrupa las notificaciones relacionadas y evita que los consumidores observen estados intermedios cuando la operación requiera consistencia atómica.

### 5.5 untracked

```ts
const count = signal(0)

effect(() => {
    const current = count()
    const snapshot = untracked(() => readOtherState())
})
```

`untracked` ejecuta una función sin registrar sus lecturas como dependencias del consumidor reactivo actual.

## 6. Ciclo de vida de las señales

Las señales no deben depender obligatoriamente de un componente.

Pueden existir como estado local, estado compartido o estado de un servicio.

- Las señales locales pertenecen al ámbito de su propietario.
- Las señales compartidas conservan su identidad mientras exista su propietario.
- Destruir un componente elimina sus suscripciones y efectos asociados.
- Destruir un componente no debe destruir señales compartidas ajenas a su ámbito.
- Los efectos deben permitir limpieza explícita.
- El runtime debe evitar referencias retenidas por suscripciones destruidas.
- El estado reactivo no debe requerir búsquedas globales de componentes.

## 7. Composición visual

La composición se realiza mediante llamadas a funciones y listas explícitas de hijos.

```ts
@component
function Dashboard() {
    return ui.column(
        gap: 16,
        padding: 24,
        children: [
            ui.text(
                "Dashboard",
                style: .title
            ),
            ui.row(
                gap: 12,
                mainAxis: .spaceBetween,
                children: [
                    ui.text("Revenue"),
                    ui.text("$24,500")
                ]
            )
        ]
    )
}
```

Reglas:

- `children` recibe nodos visuales tipados.
- Las listas de hijos pueden contener primitivas y componentes.
- Las composiciones estáticas deben poder conocerse durante la compilación.
- El compilador puede eliminar arrays y objetos temporales cuando no sean observables.
- El orden de los hijos es semánticamente significativo.
- Las primitivas de layout no acceden directamente al backend gráfico.
- La construcción de la descripción visual no debe ejecutar efectos secundarios.
- La identidad de los nodos debe permanecer estable cuando su posición lógica no cambia.

## 8. Primitivas de layout

### 8.1 Column

```ts
ui.column(
    gap: 12,
    padding: 16,
    mainAxis: .start,
    crossAxis: .stretch,
    children: [
        ui.text("Title"),
        ui.text("Description")
    ]
)
```

Organiza los hijos en el eje vertical.

### 8.2 Row

```ts
ui.row(
    gap: 8,
    mainAxis: .spaceBetween,
    crossAxis: .center,
    children: [
        ui.text("Label"),
        ui.text("Value")
    ]
)
```

Organiza los hijos en el eje horizontal.

### 8.3 Stack

```ts
ui.stack(
    alignment: .center,
    children: [
        ui.rectangle(color: .blue),
        ui.text("Overlay")
    ]
)
```

Superpone los hijos dentro de una región común.

### 8.4 Padding

```ts
ui.padding(
    all: 16,
    child: ui.text("Content")
)
```

Añade espacio interno alrededor de un único hijo.

### 8.5 SizedBox

```ts
ui.sizedBox(
    width: 120,
    height: 48,
    child: ui.text("Fixed region")
)
```

Define restricciones dimensionales.

### 8.6 Spacer

```ts
ui.row(
    children: [
        ui.text("Left"),
        ui.spacer(),
        ui.text("Right")
    ]
)
```

Consume espacio flexible dentro del layout.

## 9. Primitivas de contenido

### Text

```ts
ui.text(
    "Hello",
    style: .body,
    color: .primary,
    maxLines: 2
)
```

### Button

```ts
ui.button(
    text: "Save",
    enabled: true,
    onPress: save
)
```

### Input

```ts
const name = signal("")

ui.input(
    value: name(),
    onChange: value => name(value),
    placeholder: "Name"
)
```

### Image

```ts
ui.image(
    source: avatarUrl,
    width: 64,
    height: 64,
    fit: .cover
)
```

### ProgressIndicator

```ts
ui.progressIndicator(
    value: progress(),
    indeterminate: false
)
```

Cada primitiva tiene una firma pública tipada y una implementación en el runtime nativo.

Los valores visuales deben usar tipos dedicados cuando corresponda, en lugar de cadenas arbitrarias.

## 10. Control de flujo visual

### 10.1 Condicional

```ts
@component
function ConnectionStatus() {
    const connected = signal(false)

    return ui.column(
        children: [
            ui.when(
                condition: connected(),
                then: () => ui.text("Connected"),
                otherwise: () => ui.text("Disconnected")
            )
        ]
    )
}
```

`ui.when` crea una región condicional. Solo la rama activa pertenece al árbol visual.

El compilador y el runtime deben preservar la identidad de la rama activa y limpiar sus recursos cuando se sustituya.

### 10.2 Colecciones

```ts
type Todo = {
    id: int,
    title: str
}

@component
function TodoList() {
    const todos = signal<Todo[]>([])

    return ui.column(
        children: [
            ui.each(
                items: todos(),
                key: todo => todo.id,
                render: todo => TodoRow(
                    todo: todo
                )
            )
        ]
    )
}

@component
function TodoRow(todo: Todo) {
    return ui.text(todo.title)
}
```

Contrato:

- `ui.each` administra una región de colección.
- Cada elemento tiene una identidad estable.
- Las claves deben ser únicas dentro de la región.
- Las inserciones y eliminaciones no reconstruyen los elementos que conservan su identidad.
- Los cambios de orden conservan la identidad cuando las claves coinciden.
- La actualización de un elemento debe limitarse a sus dependencias afectadas.
- El runtime debe disponer de una política definida para claves ausentes o duplicadas.

## 11. Componentes con estado

```ts
@component
function Counter() {
    const count = signal(0)

    const doubled = computed(
        () => count() * 2
    )

    return ui.column(
        gap: 8,
        children: [
            ui.text(`Count: ${count()}`),
            ui.text(`Doubled: ${doubled()}`),
            ui.button(
                text: "Increment",
                onPress: () => count(count() + 1)
            )
        ]
    )
}
```

El componente declara el estado y devuelve una descripción visual.

Cuando `count` cambia:

1. Se invalida `doubled`.
2. Se actualizan los consumidores de ambos valores.
3. El runtime programa las operaciones necesarias.
4. El backend actualiza únicamente los nodos afectados.
5. No se reconstruyen los componentes independientes de esas señales.

El compilador puede eliminar operaciones y abstracciones intermedias si conserva la semántica observable.

## 12. Decoradores funcionales

### component

Obligatorio para componentes visuales de usuario.

```ts
@component
function Profile(name: str) {
    return ui.text(name)
}
```

### memo

Opcional; permite declarar que una unidad visual puede memorizarse bajo una política de igualdad definida.

```ts
@component
@memo
function PriceLabel(price: float) {
    return ui.text(formatCurrency(price))
}
```

`@memo` no debe cambiar la semántica del componente ni impedir las actualizaciones de señales que consume directamente.

### pure

```ts
@component
@pure
function StaticLabel(value: str) {
    return ui.text(value)
}
```

`@pure` declara que el componente no ejecuta efectos observables durante su evaluación. El compilador debe verificar o restringir esta garantía.

Los decoradores son metadatos y directivas de compilación. Su implementación no debe requerir wrappers dinámicos cuando puedan resolverse estáticamente.

## 13. Eventos

Los eventos se representan mediante funciones tipadas.

```ts
@component
function SaveButton() {
    const saving = signal(false)

    async function save() {
        if (saving()) return

        saving(true)

        try {
            await persist()
        } finally {
            saving(false)
        }
    }

    return ui.button(
        text: "Save",
        enabled: !saving(),
        onPress: save
    )
}
```

Reglas:

- Los eventos no registran dependencias reactivas por sí mismos.
- Las lecturas dentro de un manejador no crean un efecto persistente.
- Los manejadores se liberan al destruir su nodo.
- Las tareas asíncronas deben poder invalidarse cuando corresponda.
- El runtime define la propagación, cancelación y orden de eventos.
- Las actualizaciones derivadas de eventos respetan las reglas de consistencia del scheduler.

## 14. Estilos y temas

Los estilos utilizan propiedades tipadas y valores estructurados.

```ts
ui.container(
    padding: 16,
    background: Theme.colors.surface,
    radius: 12,
    child: ui.text(
        "Card",
        color: Theme.colors.onSurface
    )
)
```

La API debe proporcionar:

- Colores tipados.
- Unidades dimensionales definidas.
- Espaciado consistente.
- Tipografía y estilos de texto.
- Bordes, radios y sombras.
- Temas globales.
- Overrides locales.
- Adaptación a escalas y densidades de pantalla.

Los estilos deben normalizarse durante la compilación cuando sean estáticos.

## 15. Animación

```ts
@component
function ExpandablePanel() {
    const expanded = signal(false)

    return ui.column(
        children: [
            ui.button(
                text: "Toggle",
                onPress: () => expanded(!expanded())
            ),
            ui.animated(
                value: expanded(),
                duration: 200,
                curve: .easeInOut,
                child: ui.when(
                    condition: expanded(),
                    then: () => ui.text("Details"),
                    otherwise: () => ui.sizedBox(
                        width: 0,
                        height: 0
                    )
                )
            )
        ]
    )
}
```

El motor de animación debe separar la interpolación temporal del renderizado de componentes.

Las animaciones compatibles con el backend deben ejecutarse en el compositor o mecanismo nativo equivalente cuando sea posible.

## 16. UI IR

El compilador genera una representación intermedia especializada para interfaces.

La UI IR debe representar:

- Declaraciones de componentes.
- Identidad de instancias.
- Propiedades estáticas y dinámicas.
- Dependencias de señales.
- Regiones condicionales.
- Regiones de colección.
- Operaciones de layout.
- Suscripciones a eventos.
- Ciclo de vida y limpieza.
- Operaciones de renderizado.

La UI IR no debe depender del DOM ni de clases del runtime de una plataforma concreta.

## 17. Pipeline de compilación

1. Parsear el código Varn.
2. Construir el AST.
3. Resolver tipos y firmas.
4. Validar decoradores y composición.
5. Identificar señales y lecturas reactivas.
6. Construir el grafo de dependencias.
7. Generar UI IR.
8. Eliminar abstracciones y operaciones redundantes.
9. Generar operaciones de actualización granular.
10. Enlazar con el runtime y backend nativos.

La implementación debe reutilizar el sistema de tipos y la infraestructura del compilador Varn, evitando duplicar análisis semántico.

## 18. Runtime nativo

El runtime se organiza en subsistemas:

- Signals: almacenamiento, lectura, escritura e invalidación.
- Scheduler: planificación y agrupación de actualizaciones.
- Ownership: identidad, ámbitos y limpieza.
- UI Tree: estructura lógica de componentes.
- Layout Engine: medidas, restricciones y posicionamiento.
- Render Tree: primitivas de dibujo.
- Event System: entrada, foco y propagación.
- Text Engine: fuentes, shaping y métricas.
- GPU Backend: recursos gráficos, comandos y composición.
- Platform Backend: ventanas, teclado, ratón, tacto y accesibilidad.

El árbol lógico y el árbol de renderizado son representaciones diferentes.

Una actualización de texto no debe reconstruir el layout completo si no cambia ninguna métrica relevante. Un cambio de tamaño puede invalidar el layout de la región afectada, mientras que una animación de opacidad puede no requerir recalcularlo.

## 19. Reglas de rendimiento

- No utilizar VDOM como mecanismo obligatorio.
- No reconstruir todos los componentes ante cualquier cambio de estado.
- No crear arrays u objetos temporales si pueden eliminarse sin cambiar la semántica.
- No usar reflexión en el camino crítico de actualización.
- No introducir wrappers de decoradores innecesarios en producción.
- No recalcular layout si las restricciones y métricas relevantes no cambiaron.
- No enviar trabajo a la GPU para cambios que solo requieren operaciones de estado.
- Agrupar escrituras relacionadas.
- Mantener estables las identidades de componentes.
- Evitar retener suscripciones después de destruir una región.
- Medir por separado CPU, layout, renderizado, GPU y latencia de entrada.

Las optimizaciones deben preservar el comportamiento observable y contar con benchmarks reproducibles.

## 20. Accesibilidad y entrada

Las primitivas visuales deben exponer semántica accesible y navegación por foco.

El runtime debe contemplar:

- Roles y etiquetas accesibles.
- Orden de foco.
- Navegación mediante teclado y entrada táctil.
- Estados de controles.
- Escalado de texto.
- Lectores de pantalla.
- Eventos de entrada normalizados.
- Particularidades de accesibilidad de cada plataforma.

La accesibilidad no puede depender exclusivamente del árbol de dibujo de la GPU.

## 21. Límites de plataforma

La API común define comportamiento y tipos compartidos.

Los backends implementan las diferencias de:

- Ventanas y superficies.
- Entrada.
- Fuentes y texto.
- Accesibilidad.
- Recursos gráficos.
- Composición y sincronización.
- Gestión de ciclo de vida de la aplicación.

Las capacidades específicas de una plataforma deben declararse explícitamente. El framework no debe simular que una capacidad está disponible cuando el backend no puede implementarla.

## 22. Estructura modular

ui/
  language/
  compiler/
    ui-ir/
    analysis/
    optimization/
    codegen/
  runtime/
    signals/
    scheduler/
    ownership/
    ui-tree/
  layout/
  rendering/
    render-tree/
    gpu-backend/
  platform/
  accessibility/
  text/
  examples/

Esta estructura es una separación conceptual. Los módulos pueden integrarse en menos crates o paquetes según los límites reales de compilación y distribución de Varn.

## 23. Criterios de aceptación

La versión inicial se considera funcional cuando:

1. El compilador reconoce `@component`.
2. Las primitivas admiten argumentos nombrados con verificación estática.
3. `signal`, `computed` y `effect` funcionan con limpieza correcta.
4. Los componentes conservan su identidad durante las actualizaciones.
5. Los cambios reactivos actualizan solo las regiones dependientes.
6. Las colecciones conservan identidad mediante claves.
7. El layout se calcula con restricciones explícitas.
8. El renderizado nativo funciona sin DOM ni React.
9. El backend GPU puede dibujar las primitivas soportadas.
10. Los eventos y el foco funcionan.
11. Las suscripciones y recursos se liberan al destruir componentes.
12. Los benchmarks verifican la latencia de actualización, asignaciones y uso de CPU.

## 24. Fuera de alcance de esta especificación

- JSX.
- HTML como representación universal.
- React como runtime.
- SSR e hidratación web como arquitectura central.
- Decoradores para declarar fronteras cliente/servidor.
- Estado reactivo mediante prefijos de variables como requisito.
- Un VDOM completo como paso obligatorio.
- Reflexión dinámica en cada actualización.
- Garantías de equivalencia visual exacta entre plataformas.

## 25. Resumen

La API pública de Varn Native UI se basa en:

- `@component` para declarar componentes.
- `ui.column(...)`, `ui.row(...)` y otras primitivas para composición.
- `children: [...]` para árboles estáticos.
- `ui.when(...)` y `ui.each(...)` para regiones dinámicas.
- `signal(...)`, `computed(...)`, `effect(...)` y `batch(...)` para reactividad.
- Argumentos nombrados tipados.
- UI IR para optimización.
- Runtime nativo para identidad, layout, eventos y ciclo de vida.
- Backend GPU y adaptadores de plataforma para la presentación.

La API debe permanecer sencilla en el código fuente. La complejidad de dependencias, identidad, actualización y renderizado debe resolverse en el compilador y runtime, aprovechando las capacidades existentes de Varn.
