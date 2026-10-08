use crate::error::CliError;
use varn_core::debug_flags::DebugFlags;

fn valid_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = crate::registry::ALL.iter().map(|p| p.id()).collect();
    names.extend(["check", "all", "types", "lsp", "trace"]);
    names
}

pub fn parse_line_range(s: &str) -> Result<(u32, u32), CliError> {
    if s == "all" {
        return Ok((0, u32::MAX));
    }
    if let Some((lo_str, hi_str)) = s.split_once('-') {
        let lo = lo_str.parse::<u32>().map_err(|_| {
            CliError::usage(format!(
                "invalid line range: '{s}' (expected N, N-M, or N-)"
            ))
        })?;
        let hi = if hi_str.is_empty() {
            u32::MAX
        } else {
            hi_str.parse::<u32>().map_err(|_| {
                CliError::usage(format!(
                    "invalid line range: '{s}' (expected N, N-M, or N-)"
                ))
            })?
        };
        Ok((lo, hi))
    } else {
        let line = s
            .parse::<u32>()
            .map_err(|_| CliError::usage(format!("invalid line number: '{s}'")))?;
        Ok((line, line))
    }
}

pub fn parse_debug_flags(spec: &str) -> Result<DebugFlags, CliError> {
    let mut flags = DebugFlags::default();
    for part in spec.split(',') {
        let phase = part.trim();
        if phase.is_empty() {
            continue;
        }
        if let Some(sub) = phase.strip_prefix("tir:") {
            match sub {
                "check" => flags.tir_check = true,
                unknown => {
                    return Err(CliError::usage(format!(
                        "unknown tir debug sub-phase: '{unknown}'\n\
                             Valid sub-phases: check"
                    )));
                }
            }
        } else if let Some(sub) = phase.strip_prefix("check:") {
            match sub {
                "types" => flags.check_types = true,
                unknown => {
                    return Err(CliError::usage(format!(
                        "unknown check debug sub-phase: '{unknown}'\n\
                             Valid sub-phases: types"
                    )));
                }
            }
        } else if let Some(range_str) = phase.strip_prefix("types:") {
            flags.types = true;
            if range_str == "all" {
                flags.types_all = true;
            } else {
                flags.types_range = Some(parse_line_range(range_str)?);
            }
        } else if let Some(range_str) = phase.strip_prefix("symbols:") {
            flags.symbols = true;
            if range_str == "all" {
                flags.symbols_all = true;
            }
        } else if let Some(range_str) = phase.strip_prefix("expr:") {
            flags.expr = true;
            flags.expr_range = Some(parse_line_range(range_str)?);
        } else if let Some(sub) = phase.strip_prefix("lsp:") {
            flags.lsp = true;
            let mut sub_parts = sub.split('+').peekable();
            while let Some(head) = sub_parts.next() {
                if head == "interact" {
                    flags.lsp_interact = true;
                    continue;
                }
                if head.starts_with("interact@") {
                    let mut body = head.strip_prefix("interact@").unwrap_or("").to_owned();
                    for next in sub_parts.by_ref() {
                        body.push('+');
                        body.push_str(next);
                    }
                    flags.lsp_interact = true;
                    if let Some(path) = body.strip_prefix("file:") {
                        let text = std::fs::read_to_string(path).map_err(|e| {
                            CliError::usage(format!("cannot read interact file {path:?}: {e}"))
                        })?;
                        for (idx, raw) in text.lines().enumerate() {
                            let raw = raw.trim();
                            if raw.is_empty() || raw.starts_with('#') {
                                continue;
                            }
                            match varn_core::debug_flags::parse_step(raw) {
                                Ok(step) => flags.lsp_cursors.push(step),
                                Err(e) => {
                                    return Err(CliError::usage(format!(
                                        "invalid interact step ({path}:{idx1}): {e}",
                                        idx1 = idx + 1
                                    )));
                                }
                            }
                        }
                        continue;
                    }
                    for raw in body.split(';') {
                        let raw = raw.trim();
                        if raw.is_empty() || raw.starts_with('#') {
                            continue;
                        }
                        match varn_core::debug_flags::parse_step(raw) {
                            Ok(step) => flags.lsp_cursors.push(step),
                            Err(e) => {
                                return Err(CliError::usage(format!(
                                        "invalid interact step {raw:?}: {e}\n\
                                         Step syntax: Ln:Col [type=TEXT] [ask=v] [expect=..] (1-based; quote values with spaces; repeat ask/expect keys)"
                                    )));
                            }
                        }
                    }
                    continue;
                }
                match head {
                    "hovers" => flags.lsp_hovers = true,
                    "semantic" => flags.lsp_semantic = true,
                    "types" => flags.lsp_types = true,
                    "completions" => flags.lsp_completions = true,
                    "symbols" => flags.lsp_symbols = true,
                    "colorize" => flags.lsp_colorize = true,
                    "hints" => flags.lsp_hints = true,
                    "all" => flags.lsp_all(),
                    unknown => {
                        return Err(CliError::usage(format!(
                                "unknown lsp debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: hovers, semantic, types, completions, symbols, colorize, hints, interact[@STEP[;STEP...]] (interact@ must be last), all"
                            )));
                    }
                }
            }
        } else if let Some(sub) = phase.strip_prefix("clif:") {
            flags.clif = true;
            for sub_part in sub.split('+') {
                match sub_part {
                    "route" => flags.clif_route = true,
                    "kinds" => flags.clif_kinds = true,
                    "ir" => flags.clif_ir = true,
                    "asm" => flags.clif_asm = true,
                    "check" => flags.clif_check = true,
                    "all" => flags.clif_all(),
                    unknown => {
                        return Err(CliError::usage(format!(
                            "unknown clif debug sub-phase: '{unknown}'\n\
                                 Valid sub-phases: route, kinds, ir, asm, check, all"
                        )));
                    }
                }
            }
        } else {
            match phase {
                "check" => {
                    flags.symbols = true;
                    flags.symbols_all = true;
                    flags.types = true;
                    flags.types_all = true;
                }

                "lsp" => flags.lsp = true,
                "types" => flags.types = true,

                "trace" => flags.trace = true,

                "all" => {
                    for p in crate::registry::in_all() {
                        apply_registered(&mut flags, p.id());
                    }
                    flags.symbols_all = true;
                    flags.types = true;
                    flags.types_all = true;
                    flags.lsp = true;
                    flags.lsp_all();
                }

                "binds" => flags.binds = true,
                "expr" => flags.expr = true,
                "errors" => flags.errors = true,
                "calls" => flags.calls = true,
                "consts" => flags.consts = true,
                "info" => flags.info = true,
                unknown => match crate::registry::lookup(unknown) {
                    Some(p) => apply_registered(&mut flags, p.id()),
                    None => {
                        let names = valid_names();
                        return Err(CliError::usage(format!(
                            "unknown debug phase: '{unknown}'\n\
                                 Valid phases: {}\n\
                                 Run with --list-phases for descriptions.",
                            names.join(", ")
                        )));
                    }
                },
            }
        }
    }
    Ok(flags)
}

fn apply_registered(flags: &mut DebugFlags, id: &str) {
    match id {
        "tokens" => flags.tokens = true,
        "ast" => flags.ast = true,
        "modules" => flags.modules = true,
        "symbols" => flags.symbols = true,
        "check:types" => flags.check_types = true,
        "bytecode" => flags.bytecode = true,
        "scope" => flags.scope = true,
        "caps" => flags.cap_trace = true,
        "graph" => flags.graph = true,
        "summary" => flags.summary = true,
        "typeloss" => flags.typeloss = true,
        "tir" => flags.tir = true,
        "tiers" => flags.tiers = true,
        "bails" => flags.bails = true,
        "clif" => flags.clif_all_on(),
        "gc" => flags.gc = true,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_phase_sets_its_flag() {
        assert!(parse_debug_flags("tokens").unwrap().tokens);
        assert!(parse_debug_flags("symbols").unwrap().symbols);
        assert!(parse_debug_flags("modules").unwrap().modules);
        assert!(parse_debug_flags("scope").unwrap().scope);
        assert!(parse_debug_flags("check:types").unwrap().check_types);
        assert!(parse_debug_flags("cap").unwrap().cap_trace);
        assert!(parse_debug_flags("cap-trace").unwrap().cap_trace);
        assert!(parse_debug_flags("caps").unwrap().cap_trace);
    }

    #[test]
    fn clif_bare_is_the_phase_plus_views_without_check() {
        let f = parse_debug_flags("clif").unwrap();
        assert!(f.clif && f.clif_route && f.clif_kinds && f.clif_ir && f.clif_asm);
        assert!(!f.clif_check);
    }

    #[test]
    fn clif_all_excludes_check() {
        let f = parse_debug_flags("clif:all").unwrap();
        assert!(f.clif_route && f.clif_kinds && f.clif_ir && f.clif_asm);
        assert!(!f.clif_check);
    }

    #[test]
    fn check_group_is_symbols_and_types_not_check_types() {
        let f = parse_debug_flags("check").unwrap();
        assert!(f.symbols && f.symbols_all && f.types && f.types_all);
        assert!(!f.check_types);
        assert!(!f.binds && !f.expr);
    }

    #[test]
    fn all_is_derived_and_excludes_sweep_phases() {
        let f = parse_debug_flags("all").unwrap();
        for on in [
            f.tokens,
            f.ast,
            f.bytecode,
            f.symbols,
            f.modules,
            f.scope,
            f.graph,
            f.cap_trace,
            f.tiers,
            f.bails,
            f.summary,
            f.clif,
            f.types,
            f.lsp,
        ] {
            assert!(on);
        }
        assert!(f.symbols_all && f.types_all && f.lsp_hovers);
        for off in [f.typeloss, f.gc, f.tir, f.check_types] {
            assert!(!off);
        }
    }

    #[test]
    fn submodes_parse() {
        let f = parse_debug_flags("tir:check").unwrap();
        assert!(f.tir_check);
        let f = parse_debug_flags("symbols:all").unwrap();
        assert!(f.symbols && f.symbols_all);
    }

    #[test]
    fn unknown_and_bad_subphases_error() {
        assert!(parse_debug_flags("definitely-not-a-phase").is_err());
        assert!(parse_debug_flags("tir:nope").is_err());
        assert!(parse_debug_flags("clif:nope").is_err());
    }

    #[test]
    fn any_and_needs_execution() {
        assert!(!parse_debug_flags("").unwrap().any());
        assert!(parse_debug_flags("tokens").unwrap().any());
        assert!(!parse_debug_flags("tokens").unwrap().needs_execution());
        assert!(parse_debug_flags("gc").unwrap().needs_execution());
    }

    #[test]
    fn valid_names_covers_registry_and_groups() {
        let names = super::valid_names();
        for p in crate::registry::ALL {
            assert!(names.contains(&p.id()), "missing {}", p.id());
        }
        for g in ["check", "all", "types", "lsp", "trace"] {
            assert!(names.contains(&g), "missing {g}");
        }
    }
}
