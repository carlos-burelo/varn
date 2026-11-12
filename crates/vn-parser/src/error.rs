use vn_core::ErrorCode;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub code: ErrorCode,
    pub message: String,
}

impl ParseError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn expected(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ExpectedToken, message)
    }

    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnexpectedToken, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidStatement, message)
    }

    pub fn invalid_expression(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidExpression, message)
    }
}

pub type PResult<T> = Result<T, ParseError>;
