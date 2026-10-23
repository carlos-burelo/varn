#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessorKind {
    Get,
    Set,
}

impl AccessorKind {
    pub const ALL: [Self; 2] = [Self::Get, Self::Set];

    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Get => "get ",
            Self::Set => "set ",
        }
    }

    pub fn key(self, name: &str) -> String {
        let mut key = String::with_capacity(self.prefix().len() + name.len());
        key.push_str(self.prefix());
        key.push_str(name);
        key
    }

    pub fn with_key<R>(self, name: &str, f: impl FnOnce(&str) -> R) -> R {
        const INLINE: usize = 64;
        let prefix = self.prefix();
        let len = prefix.len() + name.len();
        if len > INLINE {
            return f(&self.key(name));
        }
        let mut buf = [0u8; INLINE];
        buf[..prefix.len()].copy_from_slice(prefix.as_bytes());
        buf[prefix.len()..len].copy_from_slice(name.as_bytes());
        match std::str::from_utf8(&buf[..len]) {
            Ok(key) => f(key),
            Err(_) => f(&self.key(name)),
        }
    }

    pub fn of_key(key: &str) -> Option<(Self, &str)> {
        Self::ALL
            .into_iter()
            .find_map(|kind| key.strip_prefix(kind.prefix()).map(|name| (kind, name)))
    }
}
