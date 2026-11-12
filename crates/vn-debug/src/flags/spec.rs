use crate::error::FlagsError;
use vn_core::debug_flags::DebugFlags;

fn valid_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = crate::registry::ALL.iter().map(|p| p.id()).collect();
    names.extend(["check", "all", "types", "lsp", "trace"]);
    names
}

pub fn parse_line_range(s: &str) -> Result<(u32, u32), FlagsError> {
    if s == "all" {
        return Ok((0, u32::MAX));
    }
    if let Some((lo_str, hi_str)) = s.split_once('-') {
        let lo = lo_str.parse::<u32>().map_err(|_| {
            FlagsError::new(format!(
                "invalid line range: '{s}' (expected N, N-M, or N-)"
            ))
        })?;
        let hi = if hi_str.is_empty() {
            u32::MAX
        } else {
            hi_str.parse::<u32>().map_err(|_| {
                FlagsError::new(format!(
                    "invalid line range: '{s}' (expected N, N-M, or N-)"
                ))
            })?
        };
        Ok((lo, hi))
    } else {
        let line = s
            .parse::<u32>()
            .map_err(|_| FlagsError::new(format!("invalid line number: '{s}'")))?;
        Ok((line, line))
    }
}

pub fn parse_debug_flags(spec: &str) -> Result<DebugFlags, FlagsError> {
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
                    return Err(FlagsError::new(format!(
                        "unknown tir debug sub-phase: '{unknown}'\n\
                             Valid sub-phases: check"
                    )));
                }
            }
        } else if let Some(sub) = phase.strip_prefix("check:") {
            match sub {
                "types" => flags.check_types = true,
                unknown => {
                    return Err(FlagsError::new(format!(
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
                            FlagsError::new(format!("cannot read interact file {path:?}: {e}"))
                        })?;
                        for (idx, raw) in text.lines().enumerate() {
                            let raw = raw.trim();
                            if raw.is_empty() || raw.starts_with('#') {
                                continue;
                            }
                            match vn_core::debug_flags::parse_step(raw) {
                                Ok(step) => flags.lsp_cursors.push(step),
                                Err(e) => {
                                    return Err(FlagsError::new(format!(
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
                        match vn_core::debug_flags::parse_step(raw) {
                            Ok(step) => flags.lsp_cursors.push(step),
                            Err(e) => {
                                return Err(FlagsError::new(format!(
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
                        return Err(FlagsError::new(format!(
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
                        return Err(FlagsError::new(format!(
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
                        return Err(FlagsError::new(format!(
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
