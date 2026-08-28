use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use varn_core::TokenKind;

use crate::document::TokenRecord;

pub fn intrinsic_or_keyword_hover(tok: &TokenRecord) -> Option<Hover> {
    match tok.kind {
        TokenKind::True => Some(make_doc_hover(
            "true: bool",
            "Literal booleano que representa el valor de verdad lógico.",
        )),
        TokenKind::False => Some(make_doc_hover(
            "false: bool",
            "Literal booleano que representa el valor de falsedad lógico.",
        )),
        TokenKind::Null => Some(make_doc_hover(
            "null",
            "Representa la ausencia intencional de cualquier valor u objeto.",
        )),
        TokenKind::Void => Some(make_doc_hover(
            "type void",
            "Indica la ausencia intencional de valor de retorno en una función o procedimiento.",
        )),
        TokenKind::Dynamic => Some(make_doc_hover(
            "type dynamic",
            "Desactiva la verificación estática de tipos para la expresión. Despachado en runtime mediante Inline Caches polimórficos.",
        )),
        TokenKind::Match => Some(make_doc_hover(
            "match (subject) { ... }",
            "Expresión condicional de coincidencia de patrones (pattern matching) exhaustiva evaluada en tiempo de ejecución.",
        )),
        TokenKind::Yield => Some(make_doc_hover(
            "yield value",
            "Suspende la ejecución del generador y emite el valor intermedio al iterador consumidor.",
        )),
        TokenKind::Await => Some(make_doc_hover(
            "await task",
            "Suspende asíncronamente la ejecución hasta que la `Task<T>` se resuelva, sin bloquear el hilo del scheduler.",
        )),
        TokenKind::Super => Some(make_doc_hover(
            "super",
            "Referencia a la clase base inmediata para invocar constructores o métodos heredados.",
        )),
        TokenKind::Identifier => match tok.lexeme.as_str() {
            "int" => Some(make_doc_hover(
                "type int",
                "Entero de 64 bits con operaciones aritméticas nativas de hardware y desbordamiento controlado.",
            )),
            "float" => Some(make_doc_hover(
                "type float",
                "Número de punto flotante de doble precisión (IEEE 754 de 64 bits).",
            )),
            "decimal" => Some(make_doc_hover(
                "type decimal",
                "Número de coma fija de 128 bits para cálculos monetarios y operaciones financieras exactas.",
            )),
            "bigint" => Some(make_doc_hover(
                "type bigint",
                "Entero de precisión arbitraria sin límite de desbordamiento en memoria heap.",
            )),
            "str" => Some(make_doc_hover(
                "type str",
                "Secuencia inmutable de texto UTF-8 optimizada mediante Small String Optimization (SSO).",
            )),
            "char" => Some(make_doc_hover(
                "type char",
                "Punto de código escalar Unicode individual de 32 bits.",
            )),
            "bool" => Some(make_doc_hover(
                "type bool",
                "Tipo de dato lógico primitivo (`true` o `false`).",
            )),
            "symbol" => Some(make_doc_hover(
                "type symbol",
                "Identificador opaco, único e inmutable a nivel de proceso.",
            )),
            "never" => Some(make_doc_hover(
                "type never",
                "Tipo fondo que representa el retorno de funciones que divergen, entran en bucle infinito o lanzan excepciones incondicionalmente.",
            )),
            "unknown" => Some(make_doc_hover(
                "type unknown",
                "Tipo seguro de entrada que requiere comprobación o estrechamiento previo antes de operar sobre él.",
            )),
            "Task" => Some(make_doc_hover(
                "type Task<T>",
                "Representación de un cómputo asíncrono gestionado por el runtime de isolates de Varn.",
            )),
            "Generator" => Some(make_doc_hover(
                "type Generator<T>",
                "Iterador perezoso que emite valores secuenciales mediante `yield`.",
            )),
            "AsyncGenerator" => Some(make_doc_hover(
                "type AsyncGenerator<T>",
                "Flujo de datos asíncrono que emite elementos mediante `yield await`.",
            )),
            "Array" => Some(make_doc_hover(
                "type Array<T>",
                "Lista secuencial contigua indexada de elementos homogéneos.",
            )),
            "Map" => Some(make_doc_hover(
                "type Map<K, V>",
                "Estructura asociativa clave-valor indexada por tabla hash.",
            )),
            "Set" => Some(make_doc_hover(
                "type Set<T>",
                "Colección no ordenada de elementos únicos sin duplicados.",
            )),
            "Option" => Some(make_doc_hover(
                "enum Option<T> { None, Some(T) }",
                "Tipo algebraico canónico para encapsular la presencia (`Some`) o ausencia (`None`) de un valor.",
            )),
            "Result" => Some(make_doc_hover(
                "enum Result<T, E> { Ok(T), Err(E) }",
                "Tipo algebraico canónico para manejo seguro de operaciones exitosas (`Ok`) o fallidas (`Err`).",
            )),
            _ => None,
        },
        TokenKind::At => Some(make_doc_hover(
            "@decorator",
            "Prefijo de anotación de decorador para enriquecer clases, métodos y funciones con metadatos de compilación.",
        )),
        _ => None,
    }
}

pub fn decorator_hover(name: &str) -> Option<Hover> {
    match name {
        "inline" => Some(make_doc_hover(
            "@inline",
            "Instruye al compilador y al JIT a expandir el cuerpo de la función directamente en el sitio de llamada para eliminar sobrecoste de llamadas.",
        )),
        "deprecated" => Some(make_doc_hover(
            "@deprecated(reason?: str)",
            "Marca el símbolo como obsoleto. El compilador y el LSP emitirán advertencias diagnósticas en sitios de uso.",
        )),
        "test" => Some(make_doc_hover(
            "@test(name?: str)",
            "Registra la función como caso de prueba automatizado para el comando `vn test`.",
        )),
        "pure" => Some(make_doc_hover(
            "@pure",
            "Declara que la función no tiene efectos secundarios observables, habilitando optimizaciones agresivas de eliminación de código muerto (DCE) y subexpresiones comunes (CSE).",
        )),
        "capability" => Some(make_doc_hover(
            "@capability(domain: str)",
            "Especifica el permiso de seguridad necesario para que un isolate pueda invocar esta función en la frontera del sistema host.",
        )),
        _ => None,
    }
}

fn make_doc_hover(sig: &str, doc: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```varn\n{}\n```\n***\n{}", sig, doc),
        }),
        range: None,
    }
}
