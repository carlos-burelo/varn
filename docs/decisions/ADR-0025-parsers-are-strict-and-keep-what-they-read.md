# ADR-0025 — Los parsers son estrictos y no pierden lo que leen

## Decisión

Todo parser de la biblioteca estándar rechaza la entrada ambigua o malformada con un error de su clase (`ParseError` o `ProtocolError`) en lugar de interpretarla, y nunca descarta ni reescribe datos que el emisor envió.

## Qué cambia

- **HTTP:** una petición con `Transfer-Encoding` y `Content-Length`, con cabeceras de enmarcado repetidas, con un codificado distinto de `chunked`, con longitud con signo o con espacio antes de `:` se rechaza. La cabecera de respuesta se codifica en un solo sitio nativo (`encode_head`) que valida nombres, valores, texto de estado y cookies, y es la única vía de la respuesta normal y de la troceada. El cliente limita número y tamaño de cabeceras y el cuerpo que corre hasta cerrar la conexión, y el buffer de lectura se compacta.
- **URL:** `Url.parse` y `Url.resolve` siguen RFC 3986 (fragmento, consulta, esquema y autoridad, en ese orden). Un esquema es obligatorio en `Url.parse`; la consulta se conserva tal cual mientras no se edite. `redirect.resolveLocation` pasa a ser `Url.resolve`.
- **WebSocket:** marcos validados contra RFC 6455 (bits reservados, longitud mínima, enmascarado según rol, límite de mensaje antes de almacenar, fragmentación, UTF-8 estricto, códigos de cierre) y apretón de manos comprobado campo a campo.
- **CSV:** BOM ignorado, cabecera duplicada y fila de distinta longitud son error, y una celda es número solo si es un número JSON canónico.
- **multipart:** parámetros leídos por nombre exacto, cuerpo en `Bytes`.
- **decimal:** el producto y la potencia fijan la escala (suma de escalas) en la VM y en el plegado de constantes con una sola función.
- **checker:** la rama falsa de `A && B` solo estrecha lo que ambos lados estrechan.

## Por qué

Cada defecto era una interpretación silenciosa: un `Content-Length` pisado, un `007` convertido en `7`, una columna perdida, un nombre de campo tomado de `filename`. En un parser de red es además una vía de ataque (contrabando de peticiones, inyección de cabeceras).

## Consecuencias aceptadas

`Url.parse` ya no acepta `host/ruta` sin esquema; el cliente WebSocket exige `ws://` o `wss://`; un CSV con filas irregulares falla en vez de rellenar con `null`.
