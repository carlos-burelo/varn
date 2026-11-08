use super::catalog::ErrorCode;
use super::diagnostic::DiagnosticKind;

impl ErrorCode {
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::UnterminatedString,
        ErrorCode::InvalidNumericLiteral,
        ErrorCode::UnknownCharacter,
        ErrorCode::UnterminatedTemplate,
        ErrorCode::UnterminatedRegex,
        ErrorCode::InvalidEscapeSequence,
        ErrorCode::UnterminatedBlockComment,
        ErrorCode::InvalidUnicodeEscape,
        ErrorCode::ExpectedToken,
        ErrorCode::UnexpectedToken,
        ErrorCode::UnterminatedParen,
        ErrorCode::UnterminatedBrace,
        ErrorCode::UnterminatedBracket,
        ErrorCode::InvalidExpression,
        ErrorCode::InvalidStatement,
        ErrorCode::InvalidTypeAnnotation,
        ErrorCode::InvalidImportPath,
        ErrorCode::InvalidDecoratorTarget,
        ErrorCode::ConstWithoutInitializer,
        ErrorCode::InvalidRegex,
        ErrorCode::TypeMismatch,
        ErrorCode::UnknownSymbol,
        ErrorCode::NotCallable,
        ErrorCode::MissingProperty,
        ErrorCode::NotAssignable,
        ErrorCode::NotIndexable,
        ErrorCode::CircularTypeReference,
        ErrorCode::InvalidTypeOperator,
        ErrorCode::TypeAnnotationRequired,
        ErrorCode::InvalidAsCast,
        ErrorCode::InvalidSatisfies,
        ErrorCode::TupleIndexOutOfBounds,
        ErrorCode::InfiniteTypeExpansion,
        ErrorCode::IntegerOverflow,
        ErrorCode::VoidValueUsed,
        ErrorCode::ForbiddenRecordGeneric,
        ErrorCode::InferOutsideConditional,
        ErrorCode::DeprecatedUse,
        ErrorCode::ImpureOperation,
        ErrorCode::ConditionNeverFalse,
        ErrorCode::NeverNull,
        ErrorCode::NotIterable,
        ErrorCode::UntypedEmptyArray,
        ErrorCode::UnusedTask,
        ErrorCode::InvalidWithEntry,
        ErrorCode::DynamicNeedsCast,
        ErrorCode::AwaitHasNoEffect,
        ErrorCode::DuplicateRecordField,
        ErrorCode::TypeArgInferenceFailed,
        ErrorCode::ConstraintViolation,
        ErrorCode::WrongTypeArgCount,
        ErrorCode::PossibleNullDereference,
        ErrorCode::NonExhaustiveMatch,
        ErrorCode::DuplicateDeclaration,
        ErrorCode::UseBeforeDeclaration,
        ErrorCode::UnreachableCode,
        ErrorCode::InvalidBreakTarget,
        ErrorCode::InvalidContinueTarget,
        ErrorCode::ReturnOutsideFunction,
        ErrorCode::InvalidThrowOperand,
        ErrorCode::DuplicateCaseLabel,
        ErrorCode::AwaitOutsideAsync,
        ErrorCode::MatchPatternTypeMismatch,
        ErrorCode::InvalidUsingTarget,
        ErrorCode::UseBeforeAssignment,
        ErrorCode::MissingReturn,
        ErrorCode::InvalidSwitchCase,
        ErrorCode::NamespaceAsValue,
        ErrorCode::MutableExport,
        ErrorCode::UninitializedField,
        ErrorCode::InvalidFinallyExit,
        ErrorCode::MissingOverride,
        ErrorCode::SpuriousOverride,
        ErrorCode::PrivateMemberAccess,
        ErrorCode::ProtectedMemberAccess,
        ErrorCode::StaticMemberOnInstance,
        ErrorCode::AbstractMethodNotImplemented,
        ErrorCode::ConstructorNotCalled,
        ErrorCode::DuplicateMemberName,
        ErrorCode::SuperOutsideMethod,
        ErrorCode::ThisOutsideInstance,
        ErrorCode::IncompatibleOverride,
        ErrorCode::InterfaceNotSatisfied,
        ErrorCode::VarianceViolation,
        ErrorCode::CapabilityViolation,
        ErrorCode::InvalidDecoratorSignature,
        ErrorCode::CompilationInternalError,
    ];

    pub fn from_number(number: u32) -> Option<ErrorCode> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.code_number() == number)
    }

    pub fn severity(self) -> DiagnosticKind {
        match self {
            ErrorCode::DeprecatedUse | ErrorCode::UnreachableCode | ErrorCode::AwaitHasNoEffect => {
                DiagnosticKind::Warning
            }
            ErrorCode::UnterminatedString
            | ErrorCode::InvalidNumericLiteral
            | ErrorCode::DuplicateRecordField
            | ErrorCode::UnknownCharacter
            | ErrorCode::UnterminatedTemplate
            | ErrorCode::UnterminatedRegex
            | ErrorCode::InvalidEscapeSequence
            | ErrorCode::UnterminatedBlockComment
            | ErrorCode::InvalidUnicodeEscape
            | ErrorCode::ExpectedToken
            | ErrorCode::UnexpectedToken
            | ErrorCode::UnterminatedParen
            | ErrorCode::UnterminatedBrace
            | ErrorCode::UnterminatedBracket
            | ErrorCode::InvalidExpression
            | ErrorCode::InvalidStatement
            | ErrorCode::InvalidTypeAnnotation
            | ErrorCode::InvalidImportPath
            | ErrorCode::InvalidDecoratorTarget
            | ErrorCode::ConstWithoutInitializer
            | ErrorCode::InvalidRegex
            | ErrorCode::TypeMismatch
            | ErrorCode::UnknownSymbol
            | ErrorCode::NotCallable
            | ErrorCode::MissingProperty
            | ErrorCode::NotAssignable
            | ErrorCode::NotIndexable
            | ErrorCode::CircularTypeReference
            | ErrorCode::InvalidTypeOperator
            | ErrorCode::TypeAnnotationRequired
            | ErrorCode::InvalidAsCast
            | ErrorCode::InvalidSatisfies
            | ErrorCode::TupleIndexOutOfBounds
            | ErrorCode::InfiniteTypeExpansion
            | ErrorCode::IntegerOverflow
            | ErrorCode::VoidValueUsed
            | ErrorCode::ForbiddenRecordGeneric
            | ErrorCode::InferOutsideConditional
            | ErrorCode::ImpureOperation
            | ErrorCode::ConditionNeverFalse
            | ErrorCode::NeverNull
            | ErrorCode::NotIterable
            | ErrorCode::UntypedEmptyArray
            | ErrorCode::UnusedTask
            | ErrorCode::InvalidWithEntry
            | ErrorCode::DynamicNeedsCast
            | ErrorCode::TypeArgInferenceFailed
            | ErrorCode::ConstraintViolation
            | ErrorCode::WrongTypeArgCount
            | ErrorCode::PossibleNullDereference
            | ErrorCode::NonExhaustiveMatch
            | ErrorCode::DuplicateDeclaration
            | ErrorCode::UseBeforeDeclaration
            | ErrorCode::InvalidBreakTarget
            | ErrorCode::InvalidContinueTarget
            | ErrorCode::ReturnOutsideFunction
            | ErrorCode::InvalidThrowOperand
            | ErrorCode::DuplicateCaseLabel
            | ErrorCode::AwaitOutsideAsync
            | ErrorCode::MatchPatternTypeMismatch
            | ErrorCode::InvalidUsingTarget
            | ErrorCode::UseBeforeAssignment
            | ErrorCode::MissingReturn
            | ErrorCode::InvalidSwitchCase
            | ErrorCode::NamespaceAsValue
            | ErrorCode::MutableExport
            | ErrorCode::UninitializedField
            | ErrorCode::InvalidFinallyExit
            | ErrorCode::MissingOverride
            | ErrorCode::SpuriousOverride
            | ErrorCode::PrivateMemberAccess
            | ErrorCode::ProtectedMemberAccess
            | ErrorCode::StaticMemberOnInstance
            | ErrorCode::AbstractMethodNotImplemented
            | ErrorCode::ConstructorNotCalled
            | ErrorCode::DuplicateMemberName
            | ErrorCode::SuperOutsideMethod
            | ErrorCode::ThisOutsideInstance
            | ErrorCode::IncompatibleOverride
            | ErrorCode::InterfaceNotSatisfied
            | ErrorCode::VarianceViolation
            | ErrorCode::CapabilityViolation
            | ErrorCode::InvalidDecoratorSignature
            | ErrorCode::CompilationInternalError => DiagnosticKind::Error,
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            ErrorCode::UnterminatedString => "A string literal reaches the end of the file without its closing quote.",
            ErrorCode::InvalidNumericLiteral => "A number literal is malformed: a prefix without digits, a misplaced `_`, an exponent without digits, or an unknown suffix.",
            ErrorCode::UnknownCharacter => "The source contains a character that is not part of Varn's syntax.",
            ErrorCode::UnterminatedTemplate => "A template literal reaches the end of the file without its closing backtick.",
            ErrorCode::UnterminatedRegex => "A regular expression literal has no closing slash.",
            ErrorCode::InvalidEscapeSequence => "A backslash escape in a literal is not one Varn defines (\\n \\t \\r \\0 \\\\ \\' \\\" \\xHH \\u{...}).",
            ErrorCode::UnterminatedBlockComment => "A block comment reaches the end of the file without its closing marker.",
            ErrorCode::InvalidUnicodeEscape => "A \\u escape does not name a Unicode scalar value.",
            ErrorCode::ExpectedToken => "The parser needed a specific token and found another one.",
            ErrorCode::UnexpectedToken => "A token appears where the grammar does not allow it.",
            ErrorCode::UnterminatedParen => "A `(` is never closed before the end of the file.",
            ErrorCode::UnterminatedBrace => "A `{` is never closed before the end of the file.",
            ErrorCode::UnterminatedBracket => "A `[` is never closed before the end of the file.",
            ErrorCode::InvalidExpression => "The construct is syntactically an expression but its form is not allowed there.",
            ErrorCode::InvalidStatement => "The construct is not a valid statement or declaration in this position.",
            ErrorCode::InvalidTypeAnnotation => "A type annotation is malformed.",
            ErrorCode::InvalidImportPath => "An import names a module that cannot be resolved, is forbidden from this layer, or has syntax errors.",
            ErrorCode::InvalidDecoratorTarget => "A decorator is applied to a declaration that does not accept it.",
            ErrorCode::ConstWithoutInitializer => "A `const` must be initialized where it is declared.",
            ErrorCode::InvalidRegex => "A regular expression literal does not compile, or would build a user class named Regex.",
            ErrorCode::TypeMismatch => "A value of one type is used where an incompatible type is required.",
            ErrorCode::UnknownSymbol => "A name or type is not declared in any visible scope.",
            ErrorCode::NotCallable => "A call is applied to a value whose type is not a function.",
            ErrorCode::MissingProperty => "The type of the receiver has no member with that name.",
            ErrorCode::NotAssignable => "The target of an assignment cannot be assigned to (constants, classes, enums, non-places).",
            ErrorCode::NotIndexable => "An index operation is applied to a type that has no indexing.",
            ErrorCode::CircularTypeReference => "A type alias refers to itself with no indirection.",
            ErrorCode::InvalidTypeOperator => "An operator is applied to operand types it does not support (for example arithmetic on a nullable).",
            ErrorCode::TypeAnnotationRequired => "Nothing determines this type: annotate it or pass the type argument explicitly.",
            ErrorCode::InvalidAsCast => "An `as` cast between types that are unrelated or cannot be converted.",
            ErrorCode::InvalidSatisfies => "An expression does not satisfy the type named by `satisfies`.",
            ErrorCode::TupleIndexOutOfBounds => "A tuple is indexed or destructured past its fixed length.",
            ErrorCode::InfiniteTypeExpansion => "Expanding a type never terminates.",
            ErrorCode::IntegerOverflow => "A constant integer expression does not fit in `int`.",
            ErrorCode::VoidValueUsed => "The result of a call that returns `void` is used as a value.",
            ErrorCode::ForbiddenRecordGeneric => "A `Record` type is written with type arguments that are not allowed.",
            ErrorCode::InferOutsideConditional => "`infer` is only meaningful inside the extends clause of a conditional type.",
            ErrorCode::DeprecatedUse => "A declaration marked deprecated is used.",
            ErrorCode::ImpureOperation => "A pure function performs an operation with side effects.",
            ErrorCode::ConditionNeverFalse => "A condition is statically always true, so the other branch is dead.",
            ErrorCode::NeverNull => "A comparison with null is made on a value whose type can never be null.",
            ErrorCode::NotIterable => "A `for ... of` or spread is applied to a type that is not iterable.",
            ErrorCode::UntypedEmptyArray => "An empty array literal has no element type: annotate the declaration.",
            ErrorCode::UnusedTask => "A Task is created and dropped; a Task only runs when awaited or spawned.",
            ErrorCode::InvalidWithEntry => "A `with` expression names a field the object does not have.",
            ErrorCode::DynamicNeedsCast => "A value of type `dynamic` is used without first casting it with `as T`.",
            ErrorCode::AwaitHasNoEffect => "`await` is applied to a value that is not a Task, so it does nothing.",
            ErrorCode::DuplicateRecordField => "A record literal gives the same field more than once, by repeating it or by spreading a record that already has it.",
            ErrorCode::TypeArgInferenceFailed => "The type arguments of a call could not be inferred.",
            ErrorCode::ConstraintViolation => "A type argument does not satisfy the constraint of its type parameter.",
            ErrorCode::WrongTypeArgCount => "A generic is given more or fewer type arguments than it declares.",
            ErrorCode::PossibleNullDereference => "A nullable value is dereferenced without a check or `?.`.",
            ErrorCode::NonExhaustiveMatch => "A `match` does not cover every possible value of its subject.",
            ErrorCode::DuplicateDeclaration => "The same name is declared twice in a scope, or a built-in name is redeclared.",
            ErrorCode::UseBeforeDeclaration => "A binding is read before its declaration is reached.",
            ErrorCode::UnreachableCode => "Statements follow a `return`, `throw`, `break` or `continue` and can never run.",
            ErrorCode::InvalidBreakTarget => "`break` appears outside a loop or switch, or names an unknown label.",
            ErrorCode::InvalidContinueTarget => "`continue` appears outside a loop, or names an unknown label.",
            ErrorCode::ReturnOutsideFunction => "`return` appears outside a function body.",
            ErrorCode::InvalidThrowOperand => "Only `Error` values and their subclasses can be thrown.",
            ErrorCode::DuplicateCaseLabel => "Two cases of a switch have the same label.",
            ErrorCode::AwaitOutsideAsync => "`await` is only valid inside an async function or at the top level.",
            ErrorCode::MatchPatternTypeMismatch => "A match pattern can never match the type of its subject, or names a missing field.",
            ErrorCode::InvalidUsingTarget => "`using` needs a resource that can be disposed.",
            ErrorCode::UseBeforeAssignment => "A variable is read before every path has assigned it.",
            ErrorCode::MissingReturn => "A function with a return type can finish without returning a value.",
            ErrorCode::InvalidSwitchCase => "A switch case has a form switch does not allow.",
            ErrorCode::NamespaceAsValue => "A namespace is used where a value is required.",
            ErrorCode::MutableExport => "A module exports a mutable binding.",
            ErrorCode::UninitializedField => "A class field is neither initialized nor assigned by every constructor path.",
            ErrorCode::InvalidFinallyExit => "A `finally` block leaves with `return`, `break` or `continue`, discarding the pending result.",
            ErrorCode::MissingOverride => "A method overrides a parent method without being marked `override`.",
            ErrorCode::SpuriousOverride => "A method is marked `override` but nothing in the parent is overridden.",
            ErrorCode::PrivateMemberAccess => "A private member is accessed from outside its class.",
            ErrorCode::ProtectedMemberAccess => "A protected member is accessed from outside its class hierarchy.",
            ErrorCode::StaticMemberOnInstance => "A static member is reached through an instance, or an instance member through the class.",
            ErrorCode::AbstractMethodNotImplemented => "A concrete class does not implement an abstract method it inherits.",
            ErrorCode::ConstructorNotCalled => "A derived constructor does not call the parent constructor.",
            ErrorCode::DuplicateMemberName => "Two members of a type share a name.",
            ErrorCode::SuperOutsideMethod => "`super` is only valid inside a method or constructor of a class.",
            ErrorCode::ThisOutsideInstance => "`this` is only valid inside a class, enum or extension.",
            ErrorCode::IncompatibleOverride => "An override changes the member signature in an incompatible way.",
            ErrorCode::InterfaceNotSatisfied => "A class declares an interface it does not fully implement.",
            ErrorCode::VarianceViolation => "A type parameter is declared `out` or `in` but is used in a position that breaks that promise.",
            ErrorCode::CapabilityViolation => "The code uses a capability the program was not granted.",
            ErrorCode::InvalidDecoratorSignature => "A decorator function does not have the signature its target requires.",
            ErrorCode::CompilationInternalError => "The compiler failed without a user-facing cause: this is a bug in Varn.",
        }
    }
}
