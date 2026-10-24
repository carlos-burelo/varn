# Gobernanza

## Roles

- **Mantenedor**: decide qué entra. Hoy, el dueño del proyecto (`@carlos-burelo`).
- **Contribuidor**: cualquiera que envíe issues, PRs o RFCs.

## Cómo se decide

| Tipo de cambio | Camino |
|---|---|
| Fix, refactor interno, std sin cambio de contrato | PR, revisión del mantenedor |
| Cambio en sintaxis, semántica, tipos o API pública de `std:*` | [RFC](rfcs/README.md) antes del PR |
| Decisión de arquitectura del compilador o runtime | ADR en [`docs/decisions/`](docs/decisions/) |

Los ADR cerrados no se reabren sin evidencia nueva. Un cambio incompatible exige
declarar ganancia, cómo se verifica y qué se borra (Ley 10 de [`AGENTS.md`](AGENTS.md)).

## Requisitos de un PR

- Un cambio por commit (Ley 9).
- La matriz de validación de [`CONTRIBUTING.md`](CONTRIBUTING.md) en verde.
- Entrada en [`CHANGELOG.md`](CHANGELOG.md) si el cambio es visible al usuario.

## Seguridad

Vulnerabilidades: ver [`SECURITY.md`](SECURITY.md). No abrir issues públicos.
