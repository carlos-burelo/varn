# vn debug — Dashboard del Compilador

`vn debug` ejecuta el pipeline completo pero detiene antes de la VM, mostrando las estructuras internas de cada fase.

Para el panorama completo de comandos, flags y ejemplos, ver [CLI_REFERENCE.md](CLI_REFERENCE.md).

## Uso

```bash
# Todas las fases (default)
vn debug archivo.vn

# Código inline
vn debug -e "function add(a: int, b: int) = a + b"

# Fases específicas
vn debug -p ast archivo.vn
vn debug -p check archivo.vn
vn debug -p bytecode archivo.vn
```

## Flag `-p` / `--phase`

| Valor | Descripción |
|-------|-------------|
| `tokens` | Tokens del lexer |
| `ast` | AST (Abstract Syntax Tree) |
| `check` | TypedAST con tipos inferidos |
| `bytecode` | Bytecode / FunctionProto |
| `all` | Todas las fases (default) |

Otros valores disponibles: `symbols`, `binds`, `types[:N]`, `expr`, `modules`, `graph`, `caps`, `scope`, `errors`, `trace`, `info` y `lsp[:sub]`.

## Fases en detalle

### `ast` — AST
Árbol sintáctico del programa. Usa marcadores `├──`/`└──` para representar la jerarquía. Los nodos hoja muestran tipo de nodo + valor.

**Cuándo usarlo:** Verificar que una construcción sintáctica parseó correctamente. Debuggear gramática nueva.

### `check` — Tipos Inferidos
AST anotado con los tipos resueltos por el type checker. Muestra firmas de funciones, tipos de expresiones, genéricos instanciados.

**Cuándo usarlo:** Entender qué tipo infirió el checker para una expresión. Debuggear errores de tipo sutiles.

```bash
vn debug -p check -e "const x: int[] = [1, 2, 3]"
```

### `bytecode` — Bytecode
Disassembly de las instrucciones generadas. Equivalente a `vn debug -p bytecode`.

**Cuándo usarlo:** Optimizar código, verificar que peephole optimizations se apliquen, contar instrucciones.

## Ejemplos

```bash
# Ver AST de una función genérica
vn debug -p ast -e "function id<T>(x: T): T = x"

# Ver tipos inferidos
vn debug -p check -e "const arr = [1, 2, 3]; print(arr.length)"

# Ver bytecode generado para una clase
vn debug -p bytecode src/models.vn

# Todas las fases en un archivo
vn debug tests/01-arithmetic.vn
```

## Diferencia con otros comandos

| Comando | Descripción |
|---------|-------------|
| `vn debug -p ast` | AST jerárquico |
| `vn debug -p check` | Tipos y SemanticDB |
| `vn debug -p bytecode` | Solo bytecode (más compacto) |
| `vn debug -p tokens` | Tokens del lexer |
| `vn run --trace` | Debug inline durante ejecución |
| `vn check` | Solo errores de tipos, sin output de estructuras |
