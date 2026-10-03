use super::definition::FunctionProto;

impl PartialEq for FunctionProto {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.arity == other.arity
            && self.export_names == other.export_names
            && self.register_count == other.register_count
            && self.has_rest == other.has_rest
            && self.is_async == other.is_async
            && self.is_generator == other.is_generator
            && self.has_this == other.has_this
            && self.upvalue_count == other.upvalue_count
            && self.cache_count == other.cache_count
            && self.chunk == other.chunk
            && self.required_caps == other.required_caps
            && self.state_size == other.state_size
    }
}

impl Eq for FunctionProto {}

impl std::hash::Hash for FunctionProto {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.arity.hash(state);
        self.export_names.hash(state);
        self.register_count.hash(state);
        self.has_rest.hash(state);
        self.is_async.hash(state);
        self.is_generator.hash(state);
        self.has_this.hash(state);
        self.upvalue_count.hash(state);
        self.cache_count.hash(state);
        self.chunk.hash(state);
        self.required_caps.hash(state);
        self.state_size.hash(state);
    }
}
