use crate::source::SourceRange;














#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriviaKind {
    
    Line,
    
    
    
    
    
    Block,
}





#[derive(Clone, Copy, Debug)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub range: SourceRange,
}
