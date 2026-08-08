# Inspección de Fases del Compilador (`vn debug`)

Este documento describe el subcomando de inspección y depuración `vn debug`, detallando todas las fases disponibles para examinar la representación interna del compilador de **Varn**.

---

## Tabla de Contenidos

- [1. Visión General](#1-visión-general)
- [2. Fases de Depuración Disponibles](#2-fases-de-depuración-disponibles)
- [3. Ejemplos de Inspección](#3-ejemplos-de-inspección)

---

## 1. Visión General

El comando `vn debug` permite a los desarrolladores del compilador e integradores de tooling inspeccionar el estado exacto del código en cada etapa de la canalización de transformación:

```bash
vn debug -p <fase> <archivo.vn>
```

---

## 2. Fases de Depuración Disponibles

| Fase | Descripción del Producto Inspeccionado |
|---|---|
| `tokens` | Lista de tokens producidos por `varn-lexer`. |
| `ast` | Árbol de Sintaxis Abstracta generado por `varn-parser`. |
| `check` | `TypedAST` con tipos inferidos por `varn-checker`. |
| `bytecode` | Opcodes desensamblados de cada `FunctionProto`. |
| `symbols` | Tabla de símbolos del ámbito global y local. |
| `binds` | Resoluciones de ámbito y bindings de variables. |
| `types` | Volcado de la SemanticDB de tipos inferidos. |
| `expr` | Evaluación de expresiones en tiempo de compilación. |
| `modules` | Grafo de dependencias e importaciones de módulos. |
| `graph` | Grafo de Flujo de Control (CFG). |
| `caps` | Capacidades de seguridad requeridas por el módulo. |
| `trace` | Traza de ejecución paso a paso instrucción por instrucción en la VM. |
| `all` | Volcado completo de todas las fases anteriores. |

---

## 3. Ejemplos de Inspección

### Inspeccionar Bytecode Desensamblado

```bash
vn debug -p bytecode mi_programa.vn
```

### Inspeccionar Tipos Inferidos

```bash
vn debug -p check mi_programa.vn
```

### Trazado Paso a Paso de Instrucción VM

```bash
vn run --trace mi_programa.vn
```
