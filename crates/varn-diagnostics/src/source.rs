#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceRange {
    pub start: SourceLocation,
    pub end: SourceLocation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceLocation {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

impl SourceLocation {
    pub fn new(line: u32, column: u32, offset: u32) -> Self {
        Self {
            offset,
            line,
            column,
        }
    }
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
        Self::zero(SourceLocation::new(line, column, offset))
    }

    /// Extend this range's end to `other`'s end, keeping this range's start.
    pub fn to(&self, other: SourceRange) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }

    /// Hull of two ranges: min start, max end.
    pub fn merge(&self, other: SourceRange) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn contains(&self, loc: SourceLocation) -> bool {
        self.start <= loc && loc <= self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}
