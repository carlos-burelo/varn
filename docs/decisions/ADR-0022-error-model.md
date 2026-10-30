# ADR 0022: Modelo de errores de la std

## Estado
Implementado (2026-10-30).

`ChannelClosed` queda fuera de la taxonomía a propósito: es la señal de control
del runtime de tareas (declarada en `runtime:task`, que no puede importar
`std:error`), no un fallo de E/S. `AssertionError` pertenece a `std:test`.

## Contexto
La std tenía dos modelos incompatibles. 90 sitios hacían
`throw new Error("NetworkError: …")`, con el *tipo* del error dentro del texto
del mensaje, y 55 usaban `StdError(code, message)` con constantes `E_*`. Los
nativos devolvían `Err("E_FS_IO|mensaje")` y la std partía la cadena por `|`
(`toStdError`): estructura codificada en texto. Resultado: nadie podía escribir
`catch (e: NetworkError)`, el código de un error dependía del formato del
mensaje, y un `message` reformulado rompía a quien lo parseara.

El lenguaje ya tenía la maquinaria correcta: `catch (e: T)` filtra por tipo (un
error que no coincide sube al `catch` exterior), `e is T` estrecha el tipo y
`try` admite varias cláusulas `catch`. La std no la usaba.

## Decisión
1. **El tipo es el discriminante.** Se captura por clase, nunca por texto.
2. **Jerarquía de clases, no un enum `kind`.** Añadir una hoja nueva no rompe a
   quien captura el padre; una variante nueva de un enum cerrado rompería todo
   `match` exhaustivo. En errores el mundo es abierto.
3. **`code` estable, `message` libre.** El `code` (`"fs.not_found"`) es contrato
   y no cambia; el `message` es para humanos y puede cambiar entre versiones.
4. **Campos tipados** (`path`, `host`, `port`, `capability`, `position`) y
   `cause` para encadenar, en vez de texto parseable.
5. **Una sola fuente de verdad** (Ley 16): la tabla canónica vive en
   `varn-core` (`StdErrorClass`, `ErrorField`). Las clases de Varn en
   `std:error` se registran con una llamada nativa que valida nombres, padres y
   códigos contra esa tabla al cargar el módulo.
6. **Los nativos lanzan errores tipados.** `NativeError` lleva la clase y los
   campos; la VM instancia la clase registrada. Se borran `toStdError`, las
   constantes `E_*` y el parseo por `|`.
7. **Sin excepciones comprobadas** (`throws` obligatorio): su coste de
   ergonomía y su interacción con genéricos y `async` superan el beneficio.
   Queda como extensión aditiva y opcional.
8. Los errores cruzan isolates con clase, `code`, campos y `cause`.

### Taxonomía
```
Error                                        (núcleo)
└ StdError            std
  ├ IoError           io
  │  ├ FsError        fs              { path }
  │  │  ├ FsNotFoundError      fs.not_found
  │  │  ├ FsPermissionError    fs.permission_denied
  │  │  ├ FsExistsError        fs.exists
  │  │  └ FsEncodingError      fs.encoding
  │  ├ BadDescriptorError      io.bad_descriptor
  │  ├ ClosedError             io.closed
  │  └ NetError       net             { host?, port? }
  │     ├ ResolveError         net.resolve
  │     ├ ConnectError         net.connect
  │     ├ TlsError             net.tls
  │     └ ProtocolError        net.protocol
  ├ CapabilityError   capability      { capability }
  ├ TimeoutError      timeout
  ├ CancelledError    cancelled
  └ ParseError        parse           { position? }
     └ DecodeError    parse.decode    { path }
```
Los argumentos inválidos usan las clases del núcleo (`TypeError`,
`RangeError`), no una clase propia de la std.

## Consecuencias
- Cambio incompatible, aceptado antes de publicar (Ley 10): desaparecen los
  códigos `E_*` y los prefijos `XxxError:` en los mensajes.
- Añadir una clase hoja es compatible hacia atrás; cambiar un `code` no lo es.
- Un diagnóstico del repositorio rechaza `throw new Error("Algo: …")` en la
  std.

## Verificación
Suite completa en los cuatro cuadrantes; `tests/296-*` fija la jerarquía, los
códigos, los campos, el filtrado por `catch (e: T)`, el cruce entre isolates y
`toJSON`; una comprobación recorre la std y falla si reaparece el patrón
`throw new Error("Xxx:")`.
