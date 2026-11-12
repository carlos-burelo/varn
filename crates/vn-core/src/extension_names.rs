#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtensionKind {
    Method,
    Getter,
    Setter,
}

impl ExtensionKind {
    pub const ALL: [Self; 3] = [Self::Method, Self::Getter, Self::Setter];

    const fn prefix(self) -> &'static str {
        match self {
            Self::Method => "__ext_",
            Self::Getter => "__extget_",
            Self::Setter => "__extset_",
        }
    }

    pub fn mangled(self, target_key: &str, member: &str) -> String {
        let mut name = String::from(self.prefix());
        for ch in target_key.chars() {
            if ch.is_ascii_alphanumeric() {
                name.push(ch);
            } else if ch == '_' {
                name.push_str("__");
            } else {
                name.push_str(&format!("_x{:x}_", u32::from(ch)));
            }
        }
        name.push('_');
        name.push_str(member);
        name
    }
}

pub fn is_extension_name(name: &str) -> bool {
    ExtensionKind::ALL
        .into_iter()
        .any(|kind| name.starts_with(kind.prefix()))
}
