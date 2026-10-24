# Gramática de Varn

Gramática sintáctica del lenguaje en EBNF, derivada de `varn-lexer` y `varn-parser`.
Describe lo que el parser **acepta**. Los errores semánticos (tipos, nombres, modificadores
inválidos para un contexto) los reporta el checker, no el parser.

Notación: `a b` secuencia · `a | b` alternativa · `[a]` opcional · `{a}` cero o más ·
`(a)` agrupación · `"x"` terminal · `/…/` descripción en prosa.

Los terminales entre comillas son tokens; el espacio en blanco y los comentarios no
aparecen en las reglas.

---

## 1. Léxico

```ebnf
Whitespace    = " " | "\t" | "\r" | "\n" ;
LineComment   = "//" { /cualquier carácter salvo "\n"/ } ;
BlockComment  = "/*" { /cualquier carácter/ } "*/" ;
DocComment    = "/**" { /cualquier carácter/ } "*/" ;

Identifier    = IdentStart { IdentPart } ;
IdentStart    = letter | "_" | /carácter Unicode alfabético/ ;
IdentPart     = IdentStart | digit ;

IntLiteral    = Digits ;
FloatLiteral  = Digits "." Digits [ Exponent ] | Digits Exponent ;
BigIntLiteral = ( Digits | BinLit | OctLit | HexLit ) "n" ;
DecimalLit    = ( Digits | Digits "." Digits ) [ Exponent ] "d" ;
BinLit        = "0" ("b"|"B") { "0" | "1" | "_" } ;
OctLit        = "0" ("o"|"O") { octdigit | "_" } ;
HexLit        = "0" ("x"|"X") { hexdigit | "_" } ;
Digits        = digit { digit | "_" } ;
Exponent      = ("e"|"E") [ "+" | "-" ] digit { digit } ;

StrLiteral    = '"' { StrChar } '"' | "'" { StrChar } "'" ;
CharLiteral   = "'" ( StrChar-único | "\u{" hexdigit+ "}" ) "'" ;
RawStrLiteral = '"""' [ newline ] { /cualquier carácter/ } '"""' ;
StrChar       = /cualquier carácter salvo la comilla de cierre/ | "\" /cualquier carácter/ ;

Template      = "`" { TemplateChar | "${" Expression "}" } "`" ;
RegexLiteral  = "/" /cuerpo/ "/" { flag } ;
```

Reglas léxicas que no caben en EBNF:

- Una cadena entre comillas simples con **exactamente un carácter** (o una escapa) es un
  `CharLiteral`; con cualquier otra longitud es un `StrLiteral`.
- Las cadenas con `"""` son crudas y multilínea; un salto de línea justo después de la
  apertura se descarta.
- `RegexLiteral` solo se reconoce donde puede empezar una expresión; en otra posición `/`
  es división.
- `1_000` y `0xFF_FF`: `_` es separador de dígitos.
- `==` y `===` producen el mismo token (igualdad), igual que `!=` y `!==`.
- `>>` y `>>>` se parten cuando cierran argumentos de tipo (`Map<K, List<V>>`).

### Palabras reservadas

```
let const var function class struct interface type enum namespace module extension on
if else switch case default while for do break continue return throw try catch finally
using with import export from as async await yield new this super delete typeof
instanceof in of void is true false null public private protected static abstract
override readonly declare native extends implements get set constructor destructor match
```

`var`, `with`, `void` y `delete` se tokenizan pero el parser los **rechaza** con un mensaje
(no existen en Varn). `_` es el marcador de descarte (`Placeholder`).

Palabras contextuales (identificadores en cualquier otro lugar): `keyof`, `infer`,
`satisfies`, `meta`.

Muchas palabras reservadas pueden usarse como nombre de identificador cuando el contexto no
es ambiguo (`get`, `set`, `async`, `await`, `yield`, `type`, `of`, `from`, `as`, `on`…) y
como nombre de propiedad o miembro tras `.`.

---

## 2. Programa y sentencias

Los `;` son opcionales: se consumen donde aparecen y no se exigen entre sentencias.
Un `[` en una línea nueva **no** continúa la expresión anterior.

```ebnf
Program      = { Statement } ;

Statement    = { DocComment } { Decorator } ( Declaration | Plain ) ;

Plain        = Block
             | ";"
             | IfStmt | WhileStmt | DoWhileStmt | ForStmt | SwitchStmt
             | ReturnStmt | BreakStmt | ContinueStmt | ThrowStmt
             | TryStmt | UsingStmt
             | Identifier ":" Statement                (* etiqueta *)
             | SeqExpression [ ";" ] ;

Block        = "{" { Statement } "}" ;

IfStmt       = "if" "(" Expression ")" Statement [ "else" Statement ] ;
WhileStmt    = "while" "(" Expression ")" Statement ;
DoWhileStmt  = "do" Statement "while" "(" Expression ")" ;

ForStmt      = "for" [ "await" ] "(" ForHead ")" Statement ;
ForHead      = [ ForInit ] ";" [ Expression ] ";" [ Expression ]
             | ( "let" | "const" ) Pattern ( "of" | "in" ) Expression ;
ForInit      = VarDecl | Expression ;

SwitchStmt   = "switch" "(" Expression ")" "{" { SwitchCase } "}" ;
SwitchCase   = ( "case" Expression | "default" ) ":" { Statement } ;

ReturnStmt   = "return" [ Expression ] ;
BreakStmt    = "break" [ Identifier ] ;
ContinueStmt = "continue" [ Identifier ] ;
ThrowStmt    = "throw" Expression ;

TryStmt      = "try" Block { CatchClause } [ "finally" Block ] ;   (* al menos un catch o finally *)
CatchClause  = "catch" [ "(" Pattern [ ":" Type ] ")" ] Block ;

UsingStmt    = [ "await" ] "using" Binding { "," Binding } ;
Binding      = Identifier [ ":" Type ] "=" Expression ;
```

Un catch con tipo unión (`catch (e: A | B)`) es el multi-catch. Un `try` con otro token
que no sea `{` tras `try` es el operador `try` de expresión (§4).

---

## 3. Declaraciones

```ebnf
Declaration  = [ "declare" ] DeclBody
             | ImportDecl | ExportDecl ;

DeclBody     = FunctionDecl | ClassDecl | InterfaceDecl | TypeAliasDecl | EnumDecl
             | NamespaceDecl | StructDecl | ExtensionDecl | VarDecl ;
```

Los decoradores solo se aceptan antes de `function`, `class` y `export` de esas dos; antes de
cualquier otra declaración el parser emite un error.

### Variables y funciones

```ebnf
VarDecl      = ( "let" | "const" ) Declarator { "," Declarator } [ ";" ] ;
Declarator   = Pattern [ ":" Type ] [ "=" Expression ] ;   (* con `declare` no hay inicializador *)

FunctionDecl = [ "async" ] "function" [ "*" ] Identifier [ TypeParams ] Params
               [ ":" Type ] ( Block | ";" ) ;                (* `;` solo con `declare` *)

Params       = "(" [ Param { "," Param } [ "," ] ] ")" ;
Param        = { ParamMod } [ "..." ] Pattern [ "?" ] [ ":" Type ] [ "=" Expression ] ;
ParamMod     = "public" | "private" | "protected" | "readonly" ;
```

### Clases

```ebnf
ClassDecl    = [ "abstract" ] "class" [ Identifier ] [ TypeParams ] [ Params ]
               [ "extends" NewCallee [ TypeArgs ] ]
               [ "implements" Type { "," Type } ]
               "{" { ClassMember } "}" ;
```

`Params` tras el nombre son los parámetros del **constructor primario**.

```ebnf
ClassMember  = { ";" } { DocComment } { Decorator } ( StaticBlock | MemberBody ) ;
StaticBlock  = "static" Block ;
MemberBody   = { MemberMod } (
                 "constructor" Params ( Block | ";" )
               | "destructor" ( Block | ";" )
               | "get" Key "(" ")" [ ":" Type ] ( Block | ";" )
               | "set" Key "(" Param ")" ( Block | ";" )
               | [ "*" ] Key [ TypeParams ] Params [ ":" Type ] ( Block | ";" )   (* método *)
               | Key [ ":" Type ] [ "=" Expression ] [ ";" ]                       (* campo *)
               ) ;
MemberMod    = "public" | "private" | "protected" | "static" | "abstract"
             | "override" | "readonly" | "declare" | "async" ;
Key          = Identifier | StrLiteral | IntLiteral | /palabra reservada/ ;
```

`let`, `const`, `var` o `function` dentro de una clase son un error: se escribe
`nombre: Tipo` o `nombre(...) { ... }`.

### Interfaces, alias, structs, namespaces

```ebnf
InterfaceDecl = "interface" Identifier [ TypeParams ]
                [ "extends" Type { "," Type } ] "{" { InterfaceMember } "}" ;
InterfaceMember
              = [ "readonly" ] [ "async" ] (
                  "[" Param "]" ":" Type [ ";" ]                       (* firma de índice *)
                | Params ":" Type [ ";" ]                              (* firma de llamada *)
                | Key [ "?" ] [ TypeParams ] Params [ ":" Type ] [ ";" ]  (* método *)
                | Key [ "?" ] [ ":" Type ] [ ";" ]                     (* propiedad *)
                ) ;

TypeAliasDecl = "type" Identifier [ TypeParams ] "=" Type [ ";" ] ;

StructDecl    = "struct" Identifier "{" { StructField } "}" ;
StructField   = Identifier ":" Type [ "=" Expression ] [ "," ] ;

NamespaceDecl = ( "namespace" | "module" ) Identifier "{" { Declaration } "}" ;
```

### Enums

```ebnf
EnumDecl     = "enum" Identifier [ TypeParams ] [ "implements" Type { "," Type } ]
               "{" [ EnumMember { "," EnumMember } [ "," ] ] [ ";" { ClassMember } ] "}" ;
EnumMember   = Identifier ( "=" Expression
                          | "(" [ EnumField { "," EnumField } ] ")" )? ;
EnumField    = [ Identifier ":" ] ( Type | Literal ) ;
```

Un miembro es simple (`Rojo`), con valor (`Rojo = 1`) o con payload (`Circulo(float)`,
`Rect(w: float, h: float)`). El payload y el valor explícito son excluyentes. Los
miembros de clase (métodos, campos) van tras un `;`.

### Extensiones

```ebnf
ExtensionDecl = "extension" [ Identifier ] "on" Type "{" { ExtensionMember } "}" ;
ExtensionMember
              = [ "async" ] [ "*" ] (
                  "get" Identifier "(" ")" [ ":" Type ] Block
                | "set" Identifier "(" Param ")" Block
                | Identifier [ TypeParams ] Params [ ":" Type ] Block ) ;
```

### Módulos

```ebnf
ImportDecl   = "import" [ "type" ] ( StrLiteral
                 | [ Identifier [ "," ] ] ( "*" "as" Identifier | ImportList ) "from" StrLiteral
                 | Identifier "from" StrLiteral ) [ ";" ] ;
ImportList   = "{" [ ImportSpec { "," ImportSpec } [ "," ] ] "}" ;
ImportSpec   = Identifier [ "as" Identifier ] ;

ExportDecl   = "export" [ "declare" ] (
                 "default" ( FunctionDecl | ClassDecl | Expression )
               | "*" [ "as" Identifier ] "from" StrLiteral
               | [ "type" ] "{" [ ExportSpec { "," ExportSpec } [ "," ] ] "}" [ "from" StrLiteral ]
               | Declaration ) [ ";" ] ;
ExportSpec   = Identifier [ "as" Identifier ] ;
```

`import.meta.main` (la única propiedad de `import.meta`) es una expresión (§4), no una declaración.

---

## 4. Expresiones

Precedencia, de **menor** a **mayor** ligadura:

| Nivel | Operadores | Asociatividad |
|---|---|---|
| 1 | `,` (secuencia, solo en `SeqExpression`) | izq. |
| 2 | `yield`, `=` `+=` `-=` `*=` `/=` `%=` `**=` `&=` `|=` `^=` `<<=` `>>=` `>>>=` `&&=` `||=` `??=` | der. |
| 3 | `? :` | der. |
| 4 | `??` | izq. |
| 5 | `||` | izq. |
| 6 | `&&` | izq. |
| 7 | `|` | izq. |
| 8 | `^` | izq. |
| 9 | `&` | izq. |
| 10 | `==` `!=` | izq. |
| 11 | `<` `>` `<=` `>=` `instanceof` `in` | izq. |
| 12 | `\|>` | izq. |
| 13 | `..` `..=` | izq. |
| 14 | `<<` `>>` `>>>` | izq. |
| 15 | `+` `-` | izq. |
| 16 | `*` `/` `%` | izq. |
| 17 | `**` | der. |
| — | `as` `is` `satisfies` (tras cualquier operando binario) | izq. |

```ebnf
SeqExpression = AssignExpr { "," AssignExpr } ;
Expression    = AssignExpr ;

AssignExpr    = YieldExpr | ArrowFunction | Conditional [ AssignOp AssignExpr ] ;
YieldExpr     = "yield" [ "*" ] [ AssignExpr ] ;
Conditional   = Binary [ "?" AssignExpr ":" AssignExpr ] ;
Binary        = Unary { BinaryOp Unary | ("as" | "is" | "satisfies") Type } ;
BinaryOp      = /operadores de los niveles 4–17/ ;

Unary         = ( "!" | "~" | "-" | "+" | "typeof" | "await" | "try" | "++" | "--" ) Unary
              | Postfix ;
```

`-9223372036854775808` es un literal entero válido (`i64::MIN`). `void` y `delete` no
existen. `try expr` propaga el error de un `Result`/`Option`/nulable (operador `?`-like).

```ebnf
Postfix       = Call [ "++" | "--" ] ;       (* sin salto de línea antes del operador *)

Call          = Primary { Trailer } ;
Trailer       = "." Name
              | "?." ( Name | "[" Expression "]" | Arguments )
              | "::" Name                                (* acceso a meta *)
              | "[" Expression "]"                       (* misma línea que el operando *)
              | "?[" Expression "]"
              | [ TypeArgs ] Arguments
              | "!"                                      (* aserción no nula *)
              | Template                                 (* template etiquetado *)
              | "with" "{" ObjectBody "}" ;              (* copia con cambios *)

Arguments     = "(" [ Argument { "," Argument } [ "," ] ] ")" ;
Argument      = [ Identifier ":" ] [ "..." ] AssignExpr ; (* argumentos con nombre *)
Name          = Identifier | /palabra reservada/ | IntLiteral ;

NewCallee     = "new" NewCallee | Primary { "." Name | "[" Expression "]" } ;
```

```ebnf
Primary       = Literal
              | Identifier | "_" | "this" | "super"
              | "(" ")"                                  (* tupla vacía / unit *)
              | "(" SeqExpression ")"
              | ArrayLiteral | ObjectLiteral | TupleLiteral | RecordLiteral
              | Template
              | "new" NewCallee [ TypeArgs ] [ Arguments ]
              | FunctionExpr | ClassExpr
              | "import" "." "meta" "." "main"
              | MatchExpr ;

Literal       = IntLiteral | FloatLiteral | BigIntLiteral | DecimalLit | BinLit | OctLit | HexLit
              | StrLiteral | RawStrLiteral | CharLiteral | RegexLiteral
              | "true" | "false" | "null" ;

ArrayLiteral  = "[" { "," | [ "..." ] AssignExpr [ "," ] } "]" ;      (* huecos permitidos *)
TupleLiteral  = "#" "[" [ AssignExpr { "," AssignExpr } [ "," ] ] "]" ;
RecordLiteral = "#" ObjectLiteral ;
ObjectLiteral = "{" ObjectBody "}" ;
ObjectBody    = { ObjectProp [ "," ] } ;
ObjectProp    = "..." AssignExpr
              | "get" PropKey "(" ")" [ ":" Type ] Block
              | "set" PropKey "(" Param ")" Block
              | [ "async" ] [ "*" ] PropKey Params [ ":" Type ] Block        (* método *)
              | PropKey ":" AssignExpr
              | Identifier ;                                                  (* abreviada *)
PropKey       = Identifier | StrLiteral | IntLiteral | "[" AssignExpr "]" | /palabra reservada/ ;

FunctionExpr  = [ "async" ] "function" [ "*" ] [ Identifier ] [ TypeParams ] Params
                [ ":" Type ] Block ;
ClassExpr     = ClassDecl ;                               (* nombre opcional *)

ArrowFunction = [ "async" ] ( Identifier | Params ) [ ":" Type ] "=>" ( Block | AssignExpr ) ;
```

El parser decide si `(` abre una flecha o una expresión entre paréntesis con una
exploración previa (`could_be_arrow`) y un intento con retroceso.

### `match`

```ebnf
MatchExpr     = "match" ( "(" Expression ")" | Binary ) "{" { MatchCase } "}" ;
MatchCase     = MatchPattern { "|" MatchPattern } [ "if" Conditional ] "=>" ( Block | Expression ) [ "," ] ;
MatchPattern  = "_"
              | Identifier "(" [ Binding { "," Binding } ] ")"          (* variante con payload posicional *)
              | Identifier "{" [ Field { "," Field } [ "," "..." ] ] "}" (* variante con payload nombrado *)
              | Identifier "." Identifier                                 (* Enum.Miembro *)
              | Identifier
              | Primary ;                                                 (* literal *)
Binding       = Identifier [ ":" MatchPattern ] | "_" ;
Field         = Identifier [ ":" MatchPattern ] ;
```

La guarda es una `Conditional`: no admite flechas ni asignaciones, para que `n if ok => …`
lea `=>` como separador del brazo.

El parser acepta patrones anidados tras `:` pero hoy solo registra el nombre del enlace.

### Patrones de destructuración

```ebnf
Pattern       = Identifier | "_"
              | "[" { Pattern [ "=" Expression ] [ "," ] } [ "..." Pattern ] "]"
              | "{" { PatProp [ "," ] } [ "..." Pattern ] "}"
              | "..." Pattern ;
PatProp       = Name ( ":" Pattern | "as" Identifier )? [ "=" Expression ] ;
```

Los huecos en patrones de array (`[a, , b]`) son un error: se usa `_`.

### Decoradores

```ebnf
Decorator     = "@" Identifier { "." Identifier } [ Arguments ] ;
```

Los decoradores no admiten argumentos de tipo.

---

## 5. Tipos

```ebnf
Type          = Identifier "is" Type                                  (* predicado de tipo *)
              | UnionType [ "extends" UnionType "?" Type ":" Type ] ; (* condicional *)

UnionType     = IntersectionType { "|" IntersectionType } ;
IntersectionType
              = ArrayType { "&" ArrayType } ;
ArrayType     = PrimaryType { "[" "]" | "[" Type "]" | "?" } ;        (* T[] , T[K] , T? *)

PrimaryType   = Name { "." Identifier } [ TypeArgs ]                  (* nombrado / genérico *)
              | "this"
              | "typeof" Unary
              | "keyof" ArrayType
              | "infer" Identifier
              | "null" | "void"
              | LiteralType
              | TemplateType
              | "(" ")" "=>" Type                                     (* función sin parámetros *)
              | "(" FnTypeParam { "," FnTypeParam } ")" "=>" Type
              | "(" Type ")"                                          (* agrupación *)
              | "[" [ Type { "," Type } ] "]"                         (* tupla *)
              | "#" "[" [ Type { "," Type } ] "]"                     (* tupla inmutable *)
              | "{" { InterfaceMember [ ";" | "," ] } "}"             (* tipo objeto *)
              | "{" [ "readonly" ] "[" Identifier "in" Type "]" [ "?" ] ":" Type "}" ;  (* mapeado *)

FnTypeParam   = [ "..." ] [ Identifier ":" ] Type ;
LiteralType   = IntLiteral | "-" IntLiteral | StrLiteral | CharLiteral | "true" | "false" ;
TemplateType  = "`" { TemplateChar | "${" Type "}" } "`" ;

TypeArgs      = "<" Type { "," Type } [ "," ] ">" ;
TypeParams    = "<" TypeParam { "," TypeParam } [ "," ] ">" ;
TypeParam     = Identifier [ "extends" UnionType ] [ "=" Type ] ;
```

Los tipos primitivos (`int`, `float`, `str`, `bool`, `char`, `decimal`, `bigint`, `dynamic`…)
son identificadores que el checker resuelve; no son palabras reservadas.
`T?` es el tipo nulable (`T | null`). Una `?` seguida de `:` en la misma línea se lee como
el `?` de un ternario, no como sufijo nulable.

---

## 6. Lo que el parser acepta y el checker rechaza

La gramática es más ancha que el lenguaje. El checker rechaza, entre otros, `get`/`set` en
literales de objeto (se declara una clase o un método), `export` de un binding no
constante y patrones de `match` que no pueden coincidir con el tipo del sujeto.
La autoridad sobre si un programa es válido es `vn check`.

---

## 7. Cosas que el parser rechaza a propósito

| Construcción | Motivo |
|---|---|
| `var` | usar `let` o `const` |
| `with (x) { … }` | usar variables explícitas |
| `void expr`, `delete x` | `_` descarta valores; `delete` no existe |
| `extension for T { … }` | sintaxis antigua; ahora `extension [Nombre] on T { … }` |
| `function` dentro de `extension` | los miembros usan sintaxis de método |
| huecos en destructuración de array | usar `_` |
| decoradores con `<T>` | los decoradores no son genéricos |
| decoradores sobre `interface`, `type`, `enum`, `namespace`, `struct`, `extension`, variables, `import` | solo `function`, `class` y sus `export` |
| inicializador en `declare let x = …` | `declare` no define valores |
| cuerpo en miembros `declare` | solo firma |
