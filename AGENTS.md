# Varn Agent Instructions

<project>

Varn Programming Language

Lenguaje compilado, estáticamente tipado, con VM register-based, runtime asíncrono, compilador propio y stdlib implementada en Rust.

</project>

<mission>

Mantener la calidad arquitectónica del compilador, VM, runtime, stdlib y herramientas asociadas.

Toda modificación debe priorizar:

* simplicidad
* cohesión
* modularidad
* rendimiento
* mantenibilidad

La deuda técnica debe evitarse incluso si requiere cambios incompatibles.

</mission>

<workspace_map>

Consultar primero:

* docs/ARCHITECTURE.md
* docs/CRATES_STATE.md

Según el dominio:

CLI y orquestación del pipeline

* crates/varn-cli (binario `vn`) — Instalado globalmente en `C:\Users\x\.cargo\bin\vn.exe` (disponible globalmente en PATH como comando `vn`)
* crates/varn-pipeline (fases: read, lex, parse, check, compile, optimize, execute)
* docs/CLI_REFERENCE.md

Lexer y Parser

* crates/varn-lexer
* crates/varn-parser

Checker

* crates/varn-checker

Compilador (AST → HIR → SSA → bytecode)

* crates/varn-compiler
* docs/COMPILER_ARCHITECTURE.md

Backend de bytecode (liveness, register allocation, post-passes)

* crates/varn-regalloc

JIT (x86-64)

* crates/varn-jit
* docs/VM_ARCHITECTURE.md

VM (intérprete, heap, GC generacional, inline caches)

* crates/varn-vm
* docs/VM_ARCHITECTURE.md

Runtime asíncrono e isolates

* crates/varn-runtime
* docs/RUNTIME_ARCHITECTURE.md

Builtins nativos

* crates/varn-builtins
* docs/LBI_ARCHITECTURE.md

Host boundary

* docs/HOST_BOUNDARY_SPEC.md

Stdlib (`std/*.vn`, bundle `.vnb`)

* std/
* crates/varn-modules
* docs/STDLIB_ARCHITECTURE.md

</workspace_map>

<core_rules>

Antes de escribir código:

1. Comprender la arquitectura existente.
2. Identificar el dominio afectado.
3. Buscar abstracciones reutilizables.
4. Diseñar el cambio.
5. Implementar.
6. Validar.
7. Actualizar documentación relevante.

Nunca comenzar implementando sin análisis previo.

Nunca introducir complejidad para resolver problemas simples.

Preferir soluciones pequeñas y locales.

Aplicar DRY y KISS.

</core_rules>

<architecture_rules>

Organizar por dominio.

Preferir:

parser/
expressions.rs
statements.rs
types.rs

vm/
dispatch.rs
frame.rs
call.rs
ic.rs

Evitar:

parser.rs
vm.rs

Cada módulo debe tener una única responsabilidad dominante.

Los límites entre frontend, checker, compiler, VM y runtime deben mantenerse estrictos.

No introducir dependencias cruzadas innecesarias entre crates.

No convertir crates especializados en contenedores genéricos de utilidades.

</architecture_rules>

<file_size_governance>

Objetivo:

Evitar god files.

Umbrales:

300 líneas  -> ideal
500 líneas  -> advertencia
700 líneas  -> refactor recomendado
1000 líneas -> refactor obligatorio

Reglas:

Antes de modificar un archivo superior a 700 líneas:

* evaluar extracción de módulos
* evaluar separación por dominio
* evaluar reducción de responsabilidades

Ninguna funcionalidad nueva debe aumentar un archivo por encima de 1000 líneas.

Preferir múltiples archivos cohesivos frente a un archivo centralizado.

</file_size_governance>

<anti_god_object>

Evitar:

* managers globales
* registries monolíticos
* enums gigantes que acumulan dominios
* dispatch centralizados en crecimiento constante
* módulos utilitarios genéricos sin dominio claro

Cuando una estructura acumule responsabilidades no relacionadas:

dividir.

</anti_god_object>

<domain_modularization>

Toda nueva funcionalidad debe pertenecer a un dominio explícito.

Ejemplos:

Correcto:

compiler/
analysis/
codegen/
lowering/

vm/
exec/
gc/
jit/
ic/

Incorrecto:

compiler.rs
vm.rs
utils.rs
helpers.rs
misc.rs

Evitar nombres genéricos.

Nombrar los módulos según su responsabilidad real.

</domain_modularization>

<performance_rules>

No asumir métricas históricas.

No afirmar mejoras de rendimiento sin medición.

Cuando una tarea involucre rendimiento:

* ejecutar benchmark
* documentar workload
* documentar configuración
* documentar impacto observado

La corrección tiene prioridad sobre la optimización.

</performance_rules>

<implementation_preferences>

Preferir:

* composición sobre acoplamiento
* tipos explícitos
* estructuras pequeñas
* APIs especializadas
* ownership claro

Evitar:

* capas de abstracción innecesarias
* configuraciones excesivamente genéricas
* duplicación estructural
* wrappers sin valor funcional

</implementation_preferences>

<validation>

Cambios de parser/checker/compiler/vm:

* Validar contra tests/main.vn. Nota: `cargo test` no es un indicador de correctitud absoluta ni la verdad definitiva de estabilidad. Las pruebas reales de estabilidad del sistema que cubren ~95% de las features son:
  1. `cargo run --release --bin vn -- bench ./tests/main.vn -v`
  2. `cargo run --release --bin vn -- run ./tests/main.vn`
* Validar también con `VARN_NO_JIT=1` para cubrir intérprete y JIT por separado (el flag ya se propaga a isolates, generadores y bench).
* Validar **las dos procedencias de la std**. El tier dev-checkout (árbol `std/`) gana por defecto en el checkout, así que el bundle embebido — el que toma todo binario distribuido — no se ejerce salvo que se fuerce:
  * árbol: `./target/release/vn.exe run ./tests/main.vn`
  * embebido: `VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn`
  Cruzar con `VARN_NO_JIT=1` da la matriz de 4 que debe estar verde. Purgar `vn cache clean` al cambiar de procedencia.

Cambios de CLI:

* validar comandos afectados
* actualizar documentación correspondiente

Cambios de arquitectura:

* actualizar documentación relevante dentro de docs/

No declarar una tarea terminada sin validación.

</validation>

<audit_procedures>

Auditoría de tamaño de archivos Rust:

Get-ChildItem -Path ".\crates" -Recurse -Filter "*.rs" |
ForEach-Object {
    [PSCustomObject]@{
        Archivo = $_.FullName
        Lineas  = (Get-Content $_.FullName | Measure-Object -Line).Lines
    }
} |
Sort-Object Lineas -Descending |
Format-Table -AutoSize

Usar periódicamente para detectar crecimiento excesivo.

</audit_procedures>

<forbidden_behaviors>

No introducir retrocompatibilidad innecesaria.

No mantener código muerto.

No añadir dependencias sin justificación.

No crear soluciones temporales sin documentarlas.

No mover lógica entre dominios sin motivo arquitectónico claro.

No ignorar arquitectura existente para resolver problemas locales.

</forbidden_behaviors>

<backend_principle>

Cuando el checker disponga de información suficiente,
esa información debe aprovecharse en compiler, VM y JIT.

Evitar arquitecturas donde los tipos solo beneficien
al frontend y no afecten al código generado.

</backend_principle>

<evolution_strategy>

Varn prioriza la simplicidad arquitectónica sobre la compatibilidad histórica.

Cuando una arquitectura existente impida avanzar, introducir complejidad innecesaria o mantener deuda técnica:

- preferir refactorización completa
- preferir reemplazo directo
- preferir breaking changes controlados

Evitar:

- sistemas duales permanentes
- rutas legacy paralelas
- capas de compatibilidad indefinidas
- adaptadores temporales convertidos en permanentes
- feature flags usados como sustituto de refactorización

Las migraciones deben tener un destino claro y una fecha de eliminación.

Git, ramas de desarrollo y validación automática son los mecanismos principales para gestionar cambios incompatibles.

La compatibilidad histórica no es un objetivo por sí mismo.

</evolution_strategy>

<replacement_over_extension>

Si un subsistema tiene defectos fundamentales:

no extenderlo.

reemplazarlo.

No construir nuevas capas sobre una base incorrecta.

Corregir la causa raíz antes de añadir funcionalidad adicional.

</replacement_over_extension>

<hardware_and_compilation_profile>

Perfil del entorno host:
- SO: Windows (x86_64-pc-windows-msvc)
- CPU: Intel Core i7-1355U (10 núcleos: 2 P-cores + 8 E-cores, 12 hilos lógicos)
- Linker: rust-lld.exe

Reglas de compilación y ejecución para el agente:
1. Aprovechar la compilación paralela de dependencias y Thin LTO (`jobs = 12`, `lto = "thin"`). Nota: Para la compilación en release del binario final, mantén `codegen-units = 1` en `[profile.release]` debido a que los marcadores de sección personalizados de MSVC (`.varn_ops$A`, `.varn_ops$C`) en `varn-builtins` requieren que el linker agrupe los símbolos en un único CGU final.
2. NUNCA ejecutar `cargo clean` a menos que sea estrictamente necesario por corrupción de artefactos de build.
3. Para ciclos rápidos de iteración y validación, utilizar `--profile quick` o compilaciones incrementales para no invalidar cachés.
4. Para benchmarks y pruebas de estabilidad final, usar `cargo run --release --bin vn -- bench ./tests/main.vn -v`.
5. TODA ejecución de ejecutables (`vn.exe`, `cargo run`, etc.) realizada por el agente DEBE utilizar explícitamente un timeout acotado (por ejemplo, `powershell -Command "..."` con un timeout máximo de 10-15s o `Wait-Process` con timeout) para prevenir esperas e hilos colgados infinitamente.
6. Regla de Timers y Espera de Compilación: NUNCA usar timers cortos (por ejemplo, 10s) para monitorear compilaciones de release del workspace completo o binario `vn.exe`. La compilación completa y enlazado con Thin LTO + `codegen-units = 1` y compilación del bundle stdlib toma aproximadamente 2 a 2.5 minutos. En su lugar, confiar en la notificación reactiva automática de finalización de background tasks o utilizar temporizadores acordes al tiempo real de build (mínimo 60-90s si se requiere liveness check) para evitar comprobaciones innecesarias en bucle.
7. Transparencia Total de Errores y Cero Hardcoding: Si un código estándar, sintaxis o comportamiento del lenguaje falla o produce un error en tiempo de compilación o runtime, NUNCA ocultarlo, maquillar el test ni aplicar workarounds silenciosos para fingir que funciona. DEBE reportarse inmediatamente al usuario detallando el error exacto, el opcode/fase afectada y la causa raíz para ser reparada en el compilador/VM.

</hardware_and_compilation_profile>

<type_system_governance_and_zero_magic_strings>

Reglas de Gobernanza de Tipos y Prohibición de Magic Strings:

1. PROHIBICIÓN ABSOLUTA DE ALIAS DE TIPOS:
   Varn tiene nombres de tipos canónicos únicos y definitivos. NUNCA introducir ni aceptar aliases de cadena para tipos en ninguna parte del compilador, VM, runtime, checker, LSP ni stdlib:
   - Cadena de texto: `str` (NUNCA `string`, NUNCA `String` salvo clase stdlib).
   - Entero: `int` (NUNCA `integer`, NUNCA `i64`, NUNCA `number`).
   - Flotante: `float` (NUNCA `f64`, NUNCA `double`, NUNCA `number`).
   - Booleano: `bool` (NUNCA `boolean`).
   - Carácter: `char` (NUNCA `character`).
   Si el agente introduce un alias sin autorización explícita del usuario, es un error de arquitectura.

2. PROHIBICIÓN ESTRICTA DE MAGIC STRINGS:
   Queda terminantemente prohibido usar listas, patrones o comparaciones con cadenas de texto literales (ej. `matches!(name, "str" | "Array" | "Map" | "int" | ...)`, `match key { "keys" => ..., "type" => ... }`) para clasificar, despachar, filtrar o excluir tipos o miembros en cualquier fase del pipeline (lexer, parser, checker, ssa/codegen, vm, runtime).
   - Todo dispatch de miembros de metanivel o intrínsecos DEBE realizarse mediante `MemberKey::from_str(...)` y un `match` exhaustivo sobre sus variantes tipadas.
   - Toda representación o nombre de tipo en runtime/checker DEBE provenir de `TypeTag::name()` o `IntrinsicType::as_str()`, NUNCA de cadenas sueltas como `"dynamic"`, `"Range"` o `"Array"`.
   
3. ÚNICA FUENTE DE VERDAD (SINGLE SOURCE OF TRUTH):
   Todo tipo primitivo, intrínseco o miembro nativo con representación dedicada en la VM o el host DEBE estar formalmente catalogado en:
   - `crates/varn-core/src/type_tag.rs` (`TypeTag`)
   - `crates/varn-core/src/intrinsics.rs` (`IntrinsicType` para tipos, `MemberKey` para propiedades/métodos como `length`, `size`, `name`, `rawValue`, `next`, `push`, `keys`, `values`, `entries`, `hasOwn`, etc.)

4. CONSULTAS SEMÁNTICAS EN LUGAR DE STRINGS:
   - Para verificar si un nombre/tipo es un primitivo/intrínseco del lenguaje: consultar `varn_core::IntrinsicType::is_intrinsic(name)` o `type.is_primitive()`.
   - Para verificar propiedades/métodos intrínsecos: comparar contra `varn_core::MemberKey::*.as_str()` o `MemberKey::from_str(...)`.
   - Para verificar si una entidad es una clase definida por el usuario con layout de campos fijos (`HeapObj::Object` con slots indexados): consultar `bind.is_user_class(name)`.
   - Para verificar si una entidad de clase proviene de las cabeceras/builtins del runtime: consultar `class_info.is_builtin_or_intrinsic`.

5. PROTOCOLO PARA AÑADIR NUEVOS TIPOS AL LENGUAJE:
   Cuando se agregue un nuevo tipo nativo o primitivo al compilador/runtime:
   a) Registrar la variante en `TypeTag` (`crates/varn-core/src/type_tag.rs`).
   b) Registrar la constante y su mapeo en `IntrinsicType` (`crates/varn-core/src/intrinsics.rs`).
   c) Si tiene representación heap dedicada en la VM, añadir la variante a `HeapObj` y sus manejadores de GC/marcado.
   d) NUNCA añadir filtros ad-hoc de cadenas de texto en checkers, optimizadores o generadores de código.

</type_system_governance_and_zero_magic_strings>