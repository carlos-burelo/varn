#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Lex,
    Parse,
    Check,
    Compile,
    Exec,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerModule {
    No,

    Graph,
}

pub trait Phase: Sync {
    fn id(&self) -> &'static str;

    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    fn title(&self) -> &'static str;

    fn stage(&self) -> Stage;

    fn per_module(&self) -> PerModule {
        PerModule::No
    }

    fn in_all(&self) -> bool {
        true
    }

    fn groups(&self) -> &'static [&'static str] {
        &[]
    }
}
