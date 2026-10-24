# Visión de sintaxis y experiencia de desarrollo (DX)

Propuestas de ergonomía y su estado, verificado contra el compilador el
2026-10-17. Una propuesta pasa a «hecha» solo con un test en `tests/` que la
ejercite. Para el resto, aplica el checklist de [`AGENTS.md`](../../AGENTS.md)
antes de implementarla.

## Objetivos

1. Menos código repetitivo sin perder claridad estática.
2. Diagnósticos que enseñan: subrayado del código y sugerencia concreta.
3. Tooling de primera: LSP sobre el mismo checker, formateador, `vn debug`.

## Estado de las propuestas

| Propuesta | Estado | Evidencia |
|---|---|---|
| Guardas en `match` | Hecha | `tests/93-match-guards-spread-predicates.vn` |
| Type guards de usuario (`v is T`) | Hecha | `tests/93-match-guards-spread-predicates.vn` |
| Constructores primarios | Hecha (modificadores `public`/`private`, no `pub`) | `tests/87-primary-constructors-nullish-assign.vn` |
| Pipeline asíncrono implícito | Pendiente | `await x \|> f(_)` aplica `await` a `x`, no al resultado |

### Guardas en `match`

```varn
const clasificacion = match (numero) {
    n if n < 0 => "Negativo",
    n if n % 2 === 0 => "Par",
    _ => "Impar"
}
```

### Type guards

```varn
function esCadena(v: dynamic): v is str {
    return typeof v === "str"
}
```

### Constructores primarios

```varn
class Usuario(public nombre: str, public edad: int, private rol: str = "guest") {}
```

### Pipeline asíncrono (pendiente)

Que `|>` espere implícitamente cada etapa `async`:

```varn
const datos = await url
    |> http.get(_)
    |> json.parse(_)
    |> procesar(_)
```

Hoy `await` liga más fuerte que `|>`: hay que escribir `await` en cada etapa.
Decidir la precedencia es un cambio de semántica (modelo → contrato entre fases
→ optimización, `AGENTS.md` §7).
