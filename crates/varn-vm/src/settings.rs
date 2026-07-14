/// How a VM executes: passed at construction, never defaulted silently.
///
/// Every `Vm` and every `ExecCtx` takes these explicitly, and each one that
/// spawns another (an isolate's worker VM, a generator's context, a task fork)
/// hands its own down. That is deliberate: when these lived as bare `bool`
/// fields defaulting to `false`, an isolate's fresh `Vm` and a generator's
/// fresh `ExecCtx` both silently reset `no_jit`, so `VARN_NO_JIT=1` claimed to
/// disable the JIT while isolate and generator code kept running JIT'd. An
/// interpreter-only run that isn't interpreter-only is worse than none: it
/// sent a debugging session chasing a memory bug that was really stale codegen.
///
/// Adding a setting here forces every construction site to decide about it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecSettings {
    /// Never enter JIT code — run everything through the interpreter.
    ///
    /// The interpreter is not a legacy path kept alive by this flag: it is the
    /// VM's base tier, and any function the JIT declines to compile
    /// (`jit_entry == None`) runs there regardless. This only closes the door
    /// into compiled code, which is what makes it useful for splitting a
    /// misbehaviour into "representation" vs "codegen".
    pub no_jit: bool,
    /// Verbose VM tracing (frames, dispatch, returns).
    pub trace: bool,
}

impl ExecSettings {
    /// The process-wide execution switches, read in exactly one place so two
    /// VMs in the same process (the bench harness's init VM, an isolate's
    /// worker) cannot end up disagreeing about them.
    pub fn from_env(trace: bool) -> Self {
        Self {
            no_jit: std::env::var_os("VARN_NO_JIT").is_some(),
            trace,
        }
    }
}
