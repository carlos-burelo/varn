
#[macro_export]
macro_rules! define_type_kinds {
    ($name:ident, $T:ty, $N:ty, $C:ty, $F:ty, $O:ty, $E:ty) => {
        pub enum $name {
            Int,
            Float,
            Str,
            Bool,
            Void,
            Null,
            Never,
            Dynamic,
            BigInt,
            Decimal,
            Char,
            Symbol,
            This,
            Array($T),
            Union($C),
            Intersection($C),
            Tuple($C),
            Named($N),
            Generic($N, $C),
            Fn($F),
            Object($O),
            Typeof($E),
        }
    };
}










