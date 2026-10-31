macro_rules! member_keys {
    ($($variant:ident => $text:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum MemberKey {
            $($variant,)*
        }

        impl MemberKey {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }

            pub fn from_str(s: &str) -> Option<Self> {
                match s {
                    $($text => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

pub const DELEGATED_ITEM: &str = "<yield*>";
pub const BOUND_SOURCE: &str = "<bound>";
const REGEX_LITERAL_BINDING_PREFIX: &str = "<regex ";

pub fn is_regex_literal_binding(name: &str) -> bool {
    name.starts_with(REGEX_LITERAL_BINDING_PREFIX)
}

pub fn is_synthetic_name(name: &str) -> bool {
    name.starts_with('<')
}

pub fn regex_literal_binding(index: usize) -> String {
    format!("{REGEX_LITERAL_BINDING_PREFIX}{index}>")
}

member_keys! {
    Tag => "__tag",
    Value0 => "value0",
    RawValue => "rawValue",
    Length => "length",
    Size => "size",
    Name => "name",
    VariantName => "__variant_name__",
    Variant => "__variant__",
    Callable => "()",
    ToString => "toString",
    ValueOf => "valueOf",
    Iter => "iter",
    IterNext => "next",
    IterClose => "close",
    Push => "push",
    Pop => "pop",
    Shift => "shift",
    Unshift => "unshift",
    Slice => "slice",
    Join => "join",
    IndexOf => "indexOf",
    Includes => "includes",
    Split => "split",
    Chars => "chars",
    ToArray => "toArray",
    Class => "class",
    Type => "type",
    Fields => "fields",
    Methods => "methods",
    Keys => "keys",
    Values => "values",
    Entries => "entries",
    HasOwn => "hasOwn",
    Repeat => "repeat",
    PadStart => "padStart",
    PadEnd => "padEnd",
    Start => "start",
    End => "end",
    StartsWith => "startsWith",
    EndsWith => "endsWith",
    CharCodeAt => "charCodeAt",
    CodePointAt => "codePointAt",
    Substring => "substring",
    Substr => "substr",
    At => "at",
    LastIndexOf => "lastIndexOf",
    CharCode => "charCode",
    Message => "message",
    Stack => "stack",
    IsStatic => "isStatic",
    IsPrivate => "isPrivate",
    Inclusive => "inclusive",
    Stride => "stride",
}
