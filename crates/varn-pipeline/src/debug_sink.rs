use varn_core::ast::{AstArena, Program};
use varn_core::debug_flags::DebugFlags;

pub trait DebugSink {
    fn lex(&self, tokens: &[varn_core::Token], lexeme_buf: &[u8], path: &str, debug: &DebugFlags);
    fn parse(
        &self,
        program: &Program,
        arena: &AstArena,
        interner: &varn_core::AtomInterner,
        debug: &DebugFlags,
    );
    fn check(
        &self,
        program: &Program,
        source: &str,
        check: &varn_sem::output::CheckResult,
        debug: &DebugFlags,
    );
    fn compile(
        &self,
        program: &Program,
        arena: &AstArena,
        check: &varn_sem::output::CheckResult,
        proto: &varn_compiler::FunctionProto,
        graph: &crate::module_precompile::ModuleGraphBuild,
        debug: &DebugFlags,
    );
}

pub struct NullSink;

impl DebugSink for NullSink {
    fn lex(
        &self,
        _tokens: &[varn_core::Token],
        _lexeme_buf: &[u8],
        _path: &str,
        _debug: &DebugFlags,
    ) {
    }
    fn parse(
        &self,
        _program: &Program,
        _arena: &AstArena,
        _interner: &varn_core::AtomInterner,
        _debug: &DebugFlags,
    ) {
    }
    fn check(
        &self,
        _program: &Program,
        _source: &str,
        _check: &varn_sem::output::CheckResult,
        _debug: &DebugFlags,
    ) {
    }
    fn compile(
        &self,
        _program: &Program,
        _arena: &AstArena,
        _check: &varn_sem::output::CheckResult,
        _proto: &varn_compiler::FunctionProto,
        _graph: &crate::module_precompile::ModuleGraphBuild,
        _debug: &DebugFlags,
    ) {
    }
}
