use super::kind::TokenKind;

impl TokenKind {
    pub fn is_keyword(self) -> bool {
        (self as u32) >= (TokenKind::Let as u32) && (self as u32) <= (TokenKind::Match as u32)
    }

    pub fn is_literal(self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            IntegerLiteral
                | FloatLiteral
                | BinaryLiteral
                | OctalLiteral
                | HexLiteral
                | BigIntLiteral
                | DecimalLiteral
                | Str
                | RawStr
                | Char
                | Template
                | TemplateHead
                | TemplateMiddle
                | TemplateTail
                | RegularExpression
                | True
                | False
                | Null
        )
    }

    pub fn starts_statement(self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            If | While
                | For
                | Do
                | Return
                | Throw
                | Try
                | Break
                | Continue
                | Switch
                | Let
                | Const
                | Var
                | Function
                | Class
                | Struct
                | Interface
                | Type
                | Enum
                | Namespace
                | Import
                | Export
                | At
                | LBrace
                | Semicolon
                | Async
                | Declare
                | Abstract
                | Using
        )
    }

    pub fn can_be_identifier(self) -> bool {
        if self == TokenKind::Identifier {
            return true;
        }

        matches!(
            self,
            TokenKind::Get
                | TokenKind::Set
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Yield
                | TokenKind::Type
                | TokenKind::Of
                | TokenKind::As
                | TokenKind::From
                | TokenKind::Static
                | TokenKind::Abstract
                | TokenKind::Override
                | TokenKind::Readonly
                | TokenKind::Declare
                | TokenKind::Native
                | TokenKind::Is
                | TokenKind::On
                | TokenKind::Namespace
                | TokenKind::Module
                | TokenKind::Extension
                | TokenKind::Constructor
                | TokenKind::Destructor
                | TokenKind::Placeholder
                | TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
        )
    }
}
