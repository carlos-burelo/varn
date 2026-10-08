use tower_lsp_f::lsp_types::{Contents, Hover, MarkupContent, MarkupKind};
use varn_core::TokenKind;

use crate::document::{DocumentState, TokenRecord};

pub fn intrinsic_or_keyword_hover(state: &DocumentState, tok: &TokenRecord) -> Option<Hover> {
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
        TokenKind::Identifier => match state.lexeme(tok) {
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
        TokenKind::EOF
        | TokenKind::IntegerLiteral
        | TokenKind::FloatLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral
        | TokenKind::HexLiteral
        | TokenKind::BigIntLiteral
        | TokenKind::Str
        | TokenKind::Char
        | TokenKind::Template
        | TokenKind::TemplateHead
        | TokenKind::TemplateMiddle
        | TokenKind::TemplateTail
        | TokenKind::RegularExpression
        | TokenKind::LParen
        | TokenKind::RParen
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::LAngle
        | TokenKind::RAngle
        | TokenKind::Semicolon
        | TokenKind::Comma
        | TokenKind::Dot
        | TokenKind::DotDot
        | TokenKind::DotDotDot
        | TokenKind::DotDotEq
        | TokenKind::Colon
        | TokenKind::ColonColon
        | TokenKind::Question
        | TokenKind::QuestionDot
        | TokenKind::QuestionLBracket
        | TokenKind::QuestionQuestion
        | TokenKind::QuestionQuestionEq
        | TokenKind::Plus
        | TokenKind::PlusPlus
        | TokenKind::PlusEq
        | TokenKind::Minus
        | TokenKind::MinusMinus
        | TokenKind::MinusEq
        | TokenKind::Star
        | TokenKind::StarStar
        | TokenKind::StarEq
        | TokenKind::StarStarEq
        | TokenKind::Slash
        | TokenKind::SlashEq
        | TokenKind::Percent
        | TokenKind::PercentEq
        | TokenKind::Amp
        | TokenKind::AmpAmp
        | TokenKind::AmpEq
        | TokenKind::AmpAmpEq
        | TokenKind::Pipe
        | TokenKind::PipePipe
        | TokenKind::PipeEq
        | TokenKind::PipePipeEq
        | TokenKind::PipeGt
        | TokenKind::Caret
        | TokenKind::CaretEq
        | TokenKind::Tilde
        | TokenKind::LtLt
        | TokenKind::LtLtEq
        | TokenKind::GtGt
        | TokenKind::GtGtEq
        | TokenKind::GtGtGt
        | TokenKind::GtGtGtEq
        | TokenKind::Eq
        | TokenKind::EqEq
        | TokenKind::EqEqEq
        | TokenKind::Bang
        | TokenKind::BangEq
        | TokenKind::BangEqEq
        | TokenKind::Lt
        | TokenKind::LtEq
        | TokenKind::Gt
        | TokenKind::GtEq
        | TokenKind::Arrow
        | TokenKind::FatArrow
        | TokenKind::Let
        | TokenKind::Const
        | TokenKind::Var
        | TokenKind::Function
        | TokenKind::Class
        | TokenKind::Struct
        | TokenKind::Interface
        | TokenKind::Type
        | TokenKind::Enum
        | TokenKind::Namespace
        | TokenKind::Module
        | TokenKind::Extension
        | TokenKind::On
        | TokenKind::If
        | TokenKind::Else
        | TokenKind::Switch
        | TokenKind::Case
        | TokenKind::Default
        | TokenKind::While
        | TokenKind::For
        | TokenKind::Do
        | TokenKind::Break
        | TokenKind::Continue
        | TokenKind::Return
        | TokenKind::Throw
        | TokenKind::Try
        | TokenKind::Catch
        | TokenKind::Finally
        | TokenKind::Using
        | TokenKind::With
        | TokenKind::Import
        | TokenKind::Export
        | TokenKind::From
        | TokenKind::As
        | TokenKind::Async
        | TokenKind::New
        | TokenKind::This
        | TokenKind::Delete
        | TokenKind::Typeof
        | TokenKind::Instanceof
        | TokenKind::In
        | TokenKind::Of
        | TokenKind::Is
        | TokenKind::Public
        | TokenKind::Private
        | TokenKind::Protected
        | TokenKind::Static
        | TokenKind::Abstract
        | TokenKind::Override
        | TokenKind::Readonly
        | TokenKind::Declare
        | TokenKind::Native
        | TokenKind::Extends
        | TokenKind::Implements
        | TokenKind::Get
        | TokenKind::Set
        | TokenKind::Constructor
        | TokenKind::Destructor
        | TokenKind::Hash
        | TokenKind::Backslash
        | TokenKind::Dollar
        | TokenKind::Backtick
        | TokenKind::Newline
        | TokenKind::Whitespace
        | TokenKind::DocComment
        | TokenKind::Placeholder
        | TokenKind::DecimalLiteral
        | TokenKind::Spawn
        | TokenKind::Parallel
        | TokenKind::Start
        | TokenKind::RawStr => None,
    }
}

pub fn decorator_hover(name: &str) -> Option<Hover> {
    match name {
        "inline" => Some(make_doc_hover(
            "@inline",
            "Garantiza la expansión del cuerpo en el sitio de llamada o falla la compilación. Solo funciones libres con un único `return`; nada de async, generadores, rest ni recursión.",
        )),
        "deprecated" => Some(make_doc_hover(
            "@deprecated(reason?: str)",
            "Marca el símbolo como obsoleto. El compilador emite advertencia `deprecated-use` en cada uso.",
        )),
        "test" => Some(make_doc_hover(
            "@test",
            "Registra la función sin parámetros como caso de prueba: `vn test` la ejecuta aislada con estado de módulo fresco.",
        )),
        "pure" => Some(make_doc_hover(
            "@pure",
            "Declara la función pura. El compilador rechaza throw, await/spawn/yield, `new`, mutación no local y llamadas a funciones no marcadas `@pure`.",
        )),
        "capability" => Some(make_doc_hover(
            "@capability(domain: str, ...)",
            "Declara la capacidad de host que la función requiere. Quien la llame debe declararla también para propagar; el toplevel tiene autoridad ambiental.",
        )),
        _ => None,
    }
}

fn make_doc_hover(sig: &str, doc: &str) -> Hover {
    Hover {
        contents: Contents::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```varn\n{}\n```\n***\n{}", sig, doc),
        }),
        range: None,
    }
}
