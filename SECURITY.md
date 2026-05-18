# Política de Seguridad

## Versiones con soporte

Varn está en desarrollo activo. Las correcciones de seguridad se aplican sobre la rama `main`.

## Qué reportar

Reporta de forma privada (ver abajo) si encuentras:

- **VM**: escape de memoria, ejecución arbitraria de código desde bytecode malicioso, corrupción del stack o heap.
- **Compilador/Checker**: crash reproducible, panic en input controlado por el usuario.
- **CLI**: inyección de comandos, path traversal en resolución de módulos o paquetes.
- **Package manager**: integrity bypass en verificación SHA256 de paquetes descargados, manipulación del lockfile.
- **Stdlib nativa**: acceso a recursos (`fs`, `net`, `sys`) sin las capabilities declaradas.
- **`.wrc`**: ejecución de bytecode corrupto o malicioso sin error.

## Qué NO es un vulnerability

- Crashes en código `.vn` que el usuario escribe (comportamiento esperado: error de runtime).
- Panics en `--debug` o `--trace` modes (herramientas de desarrollo, no superficie de ataque).
- Comportamiento indefinido en código que ya falla en type-checking.

## Cómo reportar

**No abras un issue público** si el problema no ha sido mitigado.

Envía un reporte privado a: **carlosfernandoburelo@gmail.com**

Incluye:
- Componente afectado (`varn-vm`, `varn-cli`, `varn-pm`, etc.)
- Pasos mínimos de reproducción
- Impacto esperado
- Versión de Rust y sistema operativo
- Propuesta de mitigación si la tienes

Respuesta esperada: 72 horas para acuse de recibo.
