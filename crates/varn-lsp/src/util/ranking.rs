use varn_checker::SymbolKind;

pub fn symbol_priority(kind: SymbolKind) -> u8 {
    use SymbolKind::*;
    match kind {
        Class | Struct => 0,
        Interface | Enum => 1,
        Function => 2,
        Method => 3,
        Const => 4,
        Var | Let => 5,
        Property | TypeAlias => 6,
        Namespace | Extension => 7,
        TypeParameter => 8,
        Parameter => 9,
        EnumMember => 1,
    }
}
