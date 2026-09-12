# ADR 0006: Consolidación y Pulido de la Stdlib en 5 Dominios Cohesivos

## Estado
Aprobado e Implementado.

## Contexto
Al auditar los 27 módulos de la biblioteca estándar (`std/`) de Varn, se identificó dispersión de funcionalidades afines en paquetes pequeños e independientes, duplicación en operaciones comunes (como codificaciones base64 y hex distribuidas en `std:crypto` y `std:net`), ausencia de un subsistema estándar de logging estructurado, y falta de herramientas interactivas de terminal en `std:cli`.

Para maximizar la cohesión y simplificar la experiencia de desarrollo, se aprobó la reorganización de la biblioteca estándar bajo 5 dominios fundamentales sin fragmentar el ecosistema.

## Decisiones

### 1. Dominio de Formatos y Datos (`std:encoding`)
- Se establece `std:encoding` como el paraguas canónico para la serialización y codificación de datos.
- Submódulos consolidados:
  - `std:encoding/base64`: Codificación y decodificación Base64 (`Base64.encode`, `Base64.decode`).
  - `std:encoding/hex`: Codificación y decodificación Hexadecimal (`Hex.encode`, `Hex.decode`).
  - `std:encoding/toml`: Parser y serializador TOML (`TOML.parse`, `TOML.tryParse`, `TOML.stringify`).
  - Re-exportación unificada en `std:encoding`: `JSON`, `CSV`, `Base64`, `Hex`, `TOML`.
- Retrocompatibilidad: `std:json` y `std:csv` se mantienen operativos para preservar compatibilidad con código existente.

### 2. Dominio de Terminal y Herramientas (`std:cli`)
- Consolidación del conjunto completo para CLI sin crear paquetes satélite huérfanos:
  - `std:cli/args`: Parser de argumentos posicionales y opciones con prefijo `--` y `-` (`CLI.parse`, `parseArgs`).
  - `std:cli/color`: Estilos y colores ANSI (`bold`, `red`, `green`, etc.), limpieza de códigos con `stripAnsi()`, e introspección de interactividad mediante `isInteractive()` y `isatty()`.
  - `std:cli/table`: Renderizado de tablas con alineación de columnas y soporte para textos coloreados.
  - `std:cli/prompt`: Funciones interactivas `prompt(query, defaultVal)`, `confirm(query, defaultVal)` y `password(query)`.
- Todo el conjunto es accesible directamente desde `std:cli`.

### 3. Dominio de Concurrencia y Sincronización (`std:task`)
- Centralización de primitivas de sincronización cooperativa en `std:task/sync` (re-exportadas en `std:task`):
  - `Mutex`: Exclusión mutua asíncrona mediante canales con `lock()`, `unlock()` y `withLock(action)`.
  - `Semaphore`: Control de concurrencia con capacidad configurable y `acquire()`, `release()`, `withPermit(action)`.
  - `WaitGroup`: Contador cooperativo concurrente para esperas sincronizadas de múltiples tareas (`add()`, `done()`, `wait()`).
- Evita la proliferación de un módulo separado `std:sync`.

### 4. Dominio de Diagnóstico y Logging (`std:log`)
- Se formaliza `std:log` como módulo raíz canónico de observabilidad:
  - Enumeración fuertemente tipada `LogLevel` (`Trace = 0`, `Debug = 1`, `Info = 2`, `Warn = 3`, `Error = 4`).
  - Clase `Logger` con soporte para:
    - Niveles de severidad (`trace`, `debug`, `info`, `warn`, `error`).
    - Formato de texto con marcas de tiempo ISO-8601 (`std:time`), etiquetas coloreadas (`std:cli/color`) y metadatos serializados.
    - Modo estructurado JSON para servidores y microservicios.
    - Sub-loggers jerárquicos vía `logger.child("subtarget")`.
    - Sinks configurables para desacoplar el destino de emisión (stdout, buffers de prueba o archivos).
  - Instancia singleton predeterminada `logger` y helpers globales (`info`, `warn`, `error`, `debug`, `trace`, `getLogger`).

### 5. Dominio de Colecciones (`std:collections`)
- Extensión del módulo `std:collections` para incluir estructuras de datos de alta eficiencia:
  - `PriorityQueue<T>`: Cola de prioridad / Binary Heap con soporte para comparador personalizado (Min-Heap por defecto y Max-Heap configurable).
  - `LRUCache<V>`: Caché con desalojo LRU automático al alcanzar la capacidad límite y reactivación en lecturas `get()`.
  - Coexistencia limpia con las estructuras existentes `List<T>`, `Stack<T>` y `Queue<T>`.

## Consecuencias
- **API Cohesiva y Limpia**: Los desarrolladores tienen un mapa mental claro de 5 dominios principales para datos, terminal, concurrencia, logging y colecciones.
- **Cero Dependencias Huérfanas**: No se introdujeron paquetes sueltos como `std:ansi`, `std:sync`, `std:toml` o `std:heap`.
- **Compatibilidad Garantizada**: Se mantuvieron todos los puntos de entrada existentes, pasando el 100% de la suite de pruebas (110 suites, 2408+ pruebas).
