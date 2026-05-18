# wr inspect — Dashboard del Compilador

`inspect` ejecuta el pipeline completo pero detiene antes de la VM, mostrando las estructuras internas de cada fase.

## Uso

```bash
# Todas las fases (default)
wr inspect archivo.wr

# Código inline
wr inspect -e "function add(a: int, b: int) = a + b"

# Fases específicas
wr inspect -p parse archivo.wr
wr inspect -p check archivo.wr
wr inspect -p compile archivo.wr
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
wr inspect -p check -e "const x: int[] = [1, 2, 3]"
```

### `compile` — Bytecode
Disassembly de las instrucciones generadas. Equivalente a `wr disasm` pero integrado en inspect.

**Cuándo usarlo:** Optimizar código, verificar que peephole optimizations se apliquen, contar instrucciones.

## Ejemplos

```bash
# Ver AST de una función genérica
wr inspect -p parse -e "function id<T>(x: T): T = x"

# Ver tipos inferidos
wr inspect -p check -e "const arr = [1, 2, 3]; print(arr.length)"

# Ver bytecode generado para una clase
wr inspect -p compile src/models.wr

# Todas las fases en un archivo
wr inspect tests/01-arithmetic.wr
```

## Diferencia con otros comandos

| Comando | Descripción |
|---------|-------------|
| `wr inspect -p parse` | AST jerárquico |
| `wr inspect -p check` | Tipos y SemanticDB |
| `wr inspect -p compile` | Bytecode |
| `wr disasm` | Solo bytecode (más compacto) |
| `wr run --debug parse` | Debug inline durante ejecución |
| `wr check` | Solo errores de tipos, sin output de estructuras |
