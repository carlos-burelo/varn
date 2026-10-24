use std::time::{Duration, Instant};

use varn_core::{AtomInterner, Diagnostic, DiagnosticBag};

use crate::cache::CacheLookup;
use crate::lex::Tokens;
use crate::parse::Parsed;
use crate::request::{CachePolicy, Goal, Mode, Request};
use crate::{Executable, Failure, ModuleResolver, Session, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Lex,
    Parse,
    Check,
    Lower,
    Optimize,
    Link,
}

pub enum CacheStatus {
    Bypassed,
    Hit,
    Absent,
    Stale(String),
    Unreadable(String),
    WriteFailed(String),
}

pub struct Compilation {
    pub source: Source,
    pub tokens: Option<Tokens>,
    pub parsed: Option<Parsed>,
    pub checked: Option<varn_sem::output::CheckResult>,
    pub executable: Option<Executable>,
    pub syntax: DiagnosticBag,
    pub failure: Option<Failure>,
    pub timings: Vec<(Stage, Duration)>,
    pub cache: CacheStatus,
}

impl Compilation {
    fn new(source: Source) -> Compilation {
        Compilation {
            source,
            tokens: None,
            parsed: None,
            checked: None,
            executable: None,
            syntax: DiagnosticBag::new(),
            failure: None,
            timings: Vec::new(),
            cache: CacheStatus::Bypassed,
        }
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.checked.iter().flat_map(|c| c.diagnostics.iter())
    }

    pub fn interner(&self) -> Option<&AtomInterner> {
        match (&self.checked, &self.parsed) {
            (Some(checked), _) => Some(&checked.bind.interner),
            (None, Some(parsed)) => Some(&parsed.interner),
            (None, None) => None,
        }
    }

    pub fn into_executable(self) -> Result<Executable, Failure> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        self.executable
            .ok_or_else(|| Failure::internal("the compilation stopped before the executable"))
    }

    fn timed<T>(&mut self, stage: Stage, work: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = work();
        self.timings.push((stage, start.elapsed()));
        out
    }

    fn advance_tolerant(
        &mut self,
        resolver: &ModuleResolver,
        request: Request,
    ) -> Result<(), Failure> {
        if request.goal == Goal::Executable {
            return Err(Failure::internal(
                "a tooling compilation stops at the checker",
            ));
        }
        let source = self.source.clone();
        let (tokens, lexical) = self.timed(Stage::Lex, || {
            crate::lex::scan_tolerant(source.text(), source.path())
        });
        self.syntax.extend(lexical);
        let (token_vec, lexeme_buf) = (tokens.tokens.clone(), tokens.lexeme_buf.clone());
        self.tokens = Some(tokens);
        if request.goal == Goal::Tokens {
            return Ok(());
        }

        let (parsed, syntactic) = self.timed(Stage::Parse, || {
            crate::parse::parse_tolerant(token_vec, lexeme_buf, source.path())
        });
        self.syntax.extend(syntactic);
        if request.goal == Goal::Ast {
            self.parsed = Some(parsed);
            return Ok(());
        }

        let checked = self.timed(Stage::Check, || {
            crate::check::check_tolerant(
                &parsed.program,
                &parsed.arena,
                parsed.interner.clone(),
                resolver,
            )
        });
        self.parsed = Some(parsed);
        self.checked = Some(checked);
        Ok(())
    }

    fn advance(&mut self, resolver: &ModuleResolver, request: Request) -> Result<(), Failure> {
        if request.mode == Mode::Tooling {
            return self.advance_tolerant(resolver, request);
        }
        let cacheable = request.goal == Goal::Executable
            && request.cache == CachePolicy::Use
            && matches!(self.source, Source::File { .. });
        let cache_path = cacheable.then(|| crate::cache::compile_cache_path(self.source.path()));

        if let Some(path) = &cache_path {
            match crate::cache::load_cached_graph(path, self.source.text()) {
                Ok(CacheLookup::Hit(graph)) => {
                    self.cache = CacheStatus::Hit;
                    self.executable = Some(Executable::from_graph(graph)?);
                    return Ok(());
                }
                Ok(CacheLookup::Absent) => self.cache = CacheStatus::Absent,
                Ok(CacheLookup::Stale(reason)) => self.cache = CacheStatus::Stale(reason),
                Err(e) => self.cache = CacheStatus::Unreadable(e),
            }
        }

        let source = self.source.clone();
        let tokens = self.timed(Stage::Lex, || {
            crate::lex::scan(source.text(), source.path())
        })?;
        let (token_vec, lexeme_buf) = (tokens.tokens.clone(), tokens.lexeme_buf.clone());
        self.tokens = Some(tokens);
        if request.goal == Goal::Tokens {
            return Ok(());
        }

        let parsed = self.timed(Stage::Parse, || {
            crate::parse::parse(token_vec, lexeme_buf, source.text(), source.path())
        })?;
        if request.goal == Goal::Ast {
            self.parsed = Some(parsed);
            return Ok(());
        }

        let checked = self.timed(Stage::Check, || {
            crate::check::check_program(
                &parsed.program,
                &parsed.arena,
                parsed.interner.clone(),
                source.text(),
                resolver,
            )
        });
        self.parsed = Some(parsed);
        self.checked = Some(checked?);
        if request.goal == Goal::Checked {
            return Ok(());
        }

        let (parsed, checked) = match (&self.parsed, &self.checked) {
            (Some(parsed), Some(checked)) => (parsed, checked),
            (None, _) | (_, None) => {
                return Err(Failure::internal("the checked stage left no products"));
            }
        };
        let built = crate::compile::build_executable(
            &parsed.program,
            &parsed.arena,
            source.text(),
            checked,
            resolver,
            request.measure_optimizer,
        )?;
        self.timings.push((Stage::Lower, built.lower));
        if request.measure_optimizer {
            self.timings.push((Stage::Optimize, built.optimize));
        }
        self.timings.push((Stage::Link, built.link));
        let executable = built.executable;

        if let Some(path) = &cache_path
            && let Err(e) = crate::cache::store_cached_graph(path, &executable.graph_artifact)
        {
            self.cache = CacheStatus::WriteFailed(e.to_string());
        }
        self.executable = Some(executable);
        Ok(())
    }
}

impl Session {
    pub fn compile(&self, source: Source, request: Request) -> Compilation {
        let mut compilation = Compilation::new(source);
        if let Err(failure) = compilation.advance(self.resolver(), request) {
            compilation.failure = Some(failure);
        }
        compilation
    }
}
