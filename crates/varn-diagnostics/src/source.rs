#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceRange {
    pub start: SourceLocation,
    pub end: SourceLocation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}

impl SourceRange {
    pub fn new(start: SourceLocation, end: SourceLocation) -> Self {
        Self { start, end }
    }

    pub fn zero(loc: SourceLocation) -> Self {
        Self {
            start: loc,
            end: loc,
        }
    }

    pub fn at(line: u32, column: u32, offset: u32) -> Self {
        let loc = SourceLocation {
            line,
            column,
            offset,
        };
        Self::zero(loc)
    }

    pub fn to(&self, other: SourceRange) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }
}
