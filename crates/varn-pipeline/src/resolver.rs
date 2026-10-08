use varn_resolver::DiskResolver;

pub struct Session {
    resolver: DiskResolver,
}

impl Session {
    pub fn new() -> Self {
        Self {
            resolver: DiskResolver::with_registry(varn_modules::loader::default_registry()),
        }
    }

    pub fn resolver(&self) -> &DiskResolver {
        &self.resolver
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
