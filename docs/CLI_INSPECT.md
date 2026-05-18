# vn inspect — Dashboard del Compilador

`inspect` ejecuta el pipeline completo pero detiene antes de la VM, mostrando las estructuras internas de cada fase.

## Uso

```bash
# Todas las fases (default)
vn inspect archivo.vn

# Código inline
vn inspect -e "function add(a: int, b: int) = a + b"

# Fases específicas
vn inspect -p parse archivo.vn
vn inspect -p check archivo.vn
vn inspect -p compile archivo.vn
```

## Flag `-p` / `--phases`

| Valor | Descripción |
|-------|-------------|
| `parse` | AST (Abstract Syntax Tree) |
| `check` | TypedAST con tipos inferidos |
| `compile` | Bytecode / FunctionProto |
| `all` | Todas las fases (default) |

## Fases en detalle

### `parse` — AST
Árbol sintáctico del programa. Usa marcadores `├──`/`└──` para representar la jerarquía. Los nodos hoja muestran tipo de nodo + valor.

**Cuándo usarlo:** Verificar que una construcción sintáctica parseó correctamente. Debuggear gramática nueva.

### `check` — Tipos Inferidos
AST anotado con los tipos resueltos por el type checker. Muestra firmas de funciones, tipos de expresiones, genéricos instanciados.

**Cuándo usarlo:** Entender qué tipo infirió el checker para una expresión. Debuggear errores de tipo sutiles.

```bash
vn inspect -p check -e "const x: int[] = [1, 2, 3]"
```

### `compile` — Bytecode
Disassembly de las instrucciones generadas. Equivalente a `vn disasm` pero integrado en inspect.

**Cuándo usarlo:** Optimizar código, verificar que peephole optimizations se apliquen, contar instrucciones.

## Ejemplos

```bash
# Ver AST de una función genérica
vn inspect -p parse -e "function id<T>(x: T): T = x"

# Ver tipos inferidos
vn inspect -p check -e "const arr = [1, 2, 3]; print(arr.length)"

# Ver bytecode generado para una clase
vn inspect -p compile src/models.vn

# Todas las fases en un archivo
vn inspect tests/01-arithmetic.vn
```

## Diferencia con otros comandos

| Comando | Descripción |
|---------|-------------|
| `vn inspect -p parse` | AST jerárquico |
| `vn inspect -p check` | Tipos y SemanticDB |
| `vn inspect -p compile` | Bytecode |
| `vn disasm` | Solo bytecode (más compacto) |
| `vn run --debug parse` | Debug inline durante ejecución |
| `vn check` | Solo errores de tipos, sin output de estructuras |
