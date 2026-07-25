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
| `clif` | Backend Cranelift (ROUTE/BAIL, kinds, CLIF IR, disasm x86-64) |
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

### `clif` — Backend Cranelift
Por función: decisión ROUTE/BAIL + razón, lattice de kinds, CLIF IR textual y
disasm x86-64 del código generado. Estático (no ejecuta).

Sub-fases: `clif:route`, `clif:kinds`, `clif:ir`, `clif:asm`, `clif:all`.
`clif` a secas = las cuatro.

    vn debug -p clif      -e "function f(a:int, b:int):int { return a*b+1; }"
    vn debug -p clif:asm  src/hot.vn

**Cuándo usarlo:** verificar por qué una función rutea o bailea, revisar el
lowering a CLIF, cazar bugs de codegen/regalloc.

**Limitaciones (v1):** la inspección es estática y sin heap, así que las
constantes de heap (strings, bigint, símbolos) aparecen como `null` en el IR/disasm
y las llamadas a helpers no están simbolizadas (direcciones crudas). En
consecuencia, una función cuyo único obstáculo sería una constante de heap
residente en el nursery puede mostrarse como `ROUTE` aunque en ejecución real
haría `BAIL` (`clif: nursery heap constant`) — caso raro y marcado como
"inesperado" por el propio codegen. El disasm decodifica la función `raw` (el
cuerpo) y el `wrapper` (glue de ABI) en pasadas independientes, cada una desde su
propia base, así que el relleno entre ambos ya no desincroniza al decodificador —
las instrucciones de ambos son fieles. Nota: el `wrapper` puede terminar con su
pool de constantes embebido (p.ej. las máscaras NaN-box que Cranelift emite como
rodata tras el código); esos bytes finales se muestran como pseudo-instrucciones
tras el `ret` del wrapper — son datos, no código.

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
| `vn debug -p clif` | Backend Cranelift: ROUTE/BAIL, kinds, CLIF IR, disasm |
| `vn debug -p tokens` | Tokens del lexer |
| `vn run --trace` | Debug inline durante ejecución |
| `vn check` | Solo errores de tipos, sin output de estructuras |
