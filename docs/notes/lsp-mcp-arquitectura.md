# LSP + MCP: arquitectura y desbloqueo del binario en Windows

## 1. El bloqueo

La extensión de VS Code y el MCP lanzaban `vn lsp` directamente desde
`target/{debug,release}/vn.exe` (o desde `~/.vn/bin`). Windows bloquea los datos
de un `.exe` con un proceso vivo, así que `cargo build` fallaba con "Acceso
denegado" mientras el editor estuviera abierto.

Volver a ejecutarse a sí mismo desde una copia no sirve: el cliente LSP trata la
salida del proceso que lanzó como una caída del servidor, así que el padre tiene
que seguir vivo y mantiene bloqueado su propio exe.

## 2. `vn-shadow`

`crates/varn-shadow` es un lanzador sin dependencias. Su código no cambia, así
que cargo nunca tiene que volver a enlazarlo y su bloqueo no estorba.

1. Resuelve el objetivo: `VARN_SHADOW_TARGET` o el `vn` del mismo directorio.
2. Copia el objetivo a `<dir>/.vn-shadow/<len>-<mtime>/vn.exe` (staging + rename,
   así dos arranques concurrentes no chocan) y borra las ranuras que ya nadie
   usa. La copia queda bajo el mismo directorio porque `std_root` busca `std/`
   subiendo desde `current_exe()`.
3. Lanza la copia con los mismos argumentos y stdio heredado, y devuelve su
   código de salida. El hijo hereda los pipes del cliente: si el cliente mata al
   lanzador y cierra los pipes, el servidor recibe EOF y termina.
4. En Unix hace `exec` directo; ahí no hay bloqueo.

La extensión y el MCP prefieren `vn-shadow` cuando existe junto al `vn` que
resuelven, y vigilan el `vn` original para reiniciar el servidor cuando cambia.
Basta con compilarlo una vez:

```
cargo build -p varn-shadow
```

Instalación: `scripts/install.ps1 [-Dest …]` compila `vn` y `vn-shadow` en
release, renombra los instalados a `<bin>.old-<marca>` y copia los nuevos.

## 3. Arquitectura objetivo

La práctica de rust-analyzer, gopls, deno, ruff y biome: la lógica vive en
librerías y los binarios son delgados. Un único binario de distribución es una
decisión de empaquetado, no de arquitectura.

```
varn-std-bundle  lib  build.rs que produce STDLIB_BYTES; lo embebe cualquier bin
varn-ide         lib  documentos, workspace y consultas; sin tower-lsp ni MCP
varn-lsp         lib  adaptador tower-lsp → varn-ide; `async fn serve(io)`, sin runtime propio
varn-mcp         lib  adaptador MCP → varn-ide en proceso + run/build vía varn-pipeline
vn               bin  varn-cli con features `lsp` y `mcp`
vn-shadow        bin  lanzador de desarrollo
```

Pasos, un commit cada uno:
1. Extraer `varn-std-bundle` desde `varn-cli/build.rs`.
2. `varn-lsp` expone `serve(io)`; el runtime pasa a `commands/lsp.rs`.
3. Extraer `varn-ide` desde `varn-lsp/{analysis,db,document,index,query,workspace}`.
4. `varn-mcp` en Rust sobre `varn-ide` (`vn mcp`); se borra el repo Node cuando
   cubra las mismas herramientas.
