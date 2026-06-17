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

CLI

* crates/varn-cli
* docs/CLI_REFERENCE.md

Parser

* crates/varn-parser

Checker

* crates/varn-checker

Compiler

* crates/varn-compiler
* docs/COMPILER_ARCHITECTURE.md

VM

* crates/varn-vm
* docs/VM_ARCHITECTURE.md

Runtime

* crates/varn-runtime
* docs/RUNTIME_ARCHITECTURE.md

Builtins

* crates/varn-builtins
* docs/LBI_ARCHITECTURE.md

Host boundary

* docs/HOST_BOUNDARY_SPEC.md

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

* validar contra tests/main.vn

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