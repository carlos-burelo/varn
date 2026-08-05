use std::fmt::Write;

use super::*;

const R: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";

pub fn dump_module(module: &HirModule, filename: &str) {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "\n{BOLD}{BLUE}HIR{R}{DIM} ─────────────────────────────── {filename}{R}"
    );

    dump_function(&mut out, &module.top_level, 0);

    for f in &module.functions {
        dump_function(&mut out, f, 0);
    }

    let _ = writeln!(out, "{DIM}── end: HIR ──{R}");
    eprint!("{out}");
}

fn dump_function(out: &mut String, f: &HirFunction, depth: usize) {
    let ind = indent(depth);

    let mut flags = Vec::new();
    if f.is_async {
        flags.push("async");
    }
    if f.is_generator {
        flags.push("gen");
    }
    if f.has_this {
        flags.push("has_this");
    }
    if f.has_rest {
        flags.push("has_rest");
    }
    let flags_str = if flags.is_empty() {
        String::new()
    } else {
        format!("  {DIM}[{}]{R}", flags.join(", "))
    };

    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{CYAN}{}{R}: {DIM}{}{R}", p.name, hir_ty(p.ty)))
        .collect();

    let _ = writeln!(
        out,
        "\n{ind}{BOLD}{BLUE}fn{R} {BOLD}{}{R}({}) → {DIM}{}{R}  \
         locals={DIM}{}{R}  uvs={DIM}{}{R}{flags_str}",
        f.name,
        params.join(", "),
        hir_ty(f.return_ty),
        f.locals,
        f.upvalue_count,
    );

    for stmt in &f.body {
        dump_stmt(out, stmt, depth + 1);
    }
}

fn dump_stmt(out: &mut String, stmt: &HirStmt, depth: usize) {
    let ind = indent(depth);
    match stmt {
        HirStmt::Expr(e) => {
            let _ = writeln!(out, "{ind}{}", dump_expr(e));
        }
        HirStmt::Let { local, value, ty } => {
            let _ = writeln!(
                out,
                "{ind}{YELLOW}let{R} {CYAN}l{}{R}: {DIM}{}{R} = {}",
                local.0,
                hir_ty(*ty),
                dump_expr(value)
            );
        }
        HirStmt::Assign { target, value } => {
            let _ = writeln!(out, "{ind}{} = {}", dump_binding(target), dump_expr(value));
        }
        HirStmt::SetMember {
            object,
            name,
            value,
        } => {
            let _ = writeln!(
                out,
                "{ind}{}.{CYAN}{name}{R} = {}",
                dump_expr(object),
                dump_expr(value)
            );
        }
        HirStmt::SetFixedField {
            object,
            slot,
            value,
        } => {
            let _ = writeln!(
                out,
                "{ind}{}.{CYAN}slot#{slot}{R} = {}",
                dump_expr(object),
                dump_expr(value)
            );
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            let _ = writeln!(
                out,
                "{ind}{}[{}] = {}",
                dump_expr(object),
                dump_expr(index),
                dump_expr(value)
            );
        }
        HirStmt::Return(Some(e)) => {
            let _ = writeln!(out, "{ind}{YELLOW}return{R} {}", dump_expr(e));
        }
        HirStmt::Return(None) => {
            let _ = writeln!(out, "{ind}{YELLOW}return{R}");
        }
        HirStmt::Throw(e) => {
            let _ = writeln!(out, "{ind}{YELLOW}throw{R} {}", dump_expr(e));
        }
        HirStmt::If {
            test,
            then_body,
            else_body,
        } => {
            let _ = writeln!(out, "{ind}{YELLOW}if{R} {} {{", dump_expr(test));
            for s in then_body {
                dump_stmt(out, s, depth + 1);
            }
            if !else_body.is_empty() {
                let _ = writeln!(out, "{ind}}} {YELLOW}else{R} {{");
                for s in else_body {
                    dump_stmt(out, s, depth + 1);
                }
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::While { test, body } => {
            let _ = writeln!(out, "{ind}{YELLOW}while{R} {} {{", dump_expr(test));
            for s in body {
                dump_stmt(out, s, depth + 1);
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::DoWhile { body, test } => {
            let _ = writeln!(out, "{ind}{YELLOW}do{R} {{");
            for s in body {
                dump_stmt(out, s, depth + 1);
            }
            let _ = writeln!(out, "{ind}}} {YELLOW}while{R} {}", dump_expr(test));
        }
        HirStmt::ForClassic { test, update, body } => {
            let _ = writeln!(out, "{ind}{YELLOW}for{R} ({}; update) {{", dump_expr(test));
            for s in body {
                dump_stmt(out, s, depth + 1);
            }
            let _ = writeln!(out, "{ind}  {DIM}-- update --{R}");
            for s in update {
                dump_stmt(out, s, depth + 2);
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::ForOf {
            var,
            iterable,
            body,
            is_await,
        } => {
            let kw = if *is_await { "for await" } else { "for" };
            let _ = writeln!(
                out,
                "{ind}{YELLOW}{kw}{R} ({CYAN}l{}{R} {YELLOW}of{R} {}) {{",
                var.0,
                dump_expr(iterable)
            );
            for s in body {
                dump_stmt(out, s, depth + 1);
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::ForIn { var, object, body } => {
            let _ = writeln!(
                out,
                "{ind}{YELLOW}for{R} ({CYAN}l{}{R} {YELLOW}in{R} {}) {{",
                var.0,
                dump_expr(object)
            );
            for s in body {
                dump_stmt(out, s, depth + 1);
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::Switch { disc, cases } => {
            let _ = writeln!(out, "{ind}{YELLOW}switch{R} ({}) {{", dump_expr(disc));
            for case in cases {
                match &case.test {
                    Some(t) => {
                        let _ = writeln!(out, "{}  {YELLOW}case{R} {}:", ind, dump_expr(t));
                    }
                    None => {
                        let _ = writeln!(out, "{}  {YELLOW}default{R}:", ind);
                    }
                }
                for s in &case.body {
                    dump_stmt(out, s, depth + 2);
                }
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::Break => {
            let _ = writeln!(out, "{ind}{YELLOW}break{R}");
        }
        HirStmt::Continue => {
            let _ = writeln!(out, "{ind}{YELLOW}continue{R}");
        }
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            let _ = writeln!(out, "{ind}{YELLOW}try{R} {{");
            for s in block {
                dump_stmt(out, s, depth + 1);
            }
            if let Some(c) = catch {
                let param_str = match c.param {
                    Some(id) => format!(" ({CYAN}l{}{R})", id.0),
                    None => String::new(),
                };
                let _ = writeln!(out, "{ind}}} {YELLOW}catch{param_str}{R} {{");
                for s in &c.body {
                    dump_stmt(out, s, depth + 1);
                }
            }
            if let Some(fin) = finally {
                let _ = writeln!(out, "{ind}}} {YELLOW}finally{R} {{");
                for s in fin {
                    dump_stmt(out, s, depth + 1);
                }
            }
            let _ = writeln!(out, "{ind}}}");
        }
        HirStmt::Import {
            source,
            is_type,
            specs,
        } => {
            if *is_type {
                let _ = writeln!(
                    out,
                    "{ind}{YELLOW}import type{R} from {GREEN}\"{source}\"{R}"
                );
            } else if specs.is_empty() {
                let _ = writeln!(out, "{ind}{YELLOW}import{R} {GREEN}\"{source}\"{R}");
            } else {
                let names: Vec<String> = specs
                    .iter()
                    .map(|s| format!("{CYAN}{}{R}", s.local))
                    .collect();
                let _ = writeln!(
                    out,
                    "{ind}{YELLOW}import{R} {{ {} }} from {GREEN}\"{source}\"{R}",
                    names.join(", ")
                );
            }
        }
        HirStmt::StoreExport { name, slot } => {
            let _ = writeln!(
                out,
                "{ind}{YELLOW}export{R} {CYAN}{name}{R} → slot[{DIM}{slot}{R}]"
            );
        }
        HirStmt::ExportNamed { specifiers, source } => {
            let src_str = source
                .as_ref()
                .map(|s| format!(" from \"{s}\""))
                .unwrap_or_default();
            let specs: Vec<String> = specifiers
                .iter()
                .map(|s| format!("{} as {}", s.local, s.exported))
                .collect();
            let _ = writeln!(
                out,
                "{ind}{YELLOW}export{R} {{{}}}{src_str}",
                specs.join(", ")
            );
        }
        HirStmt::ExportAll {
            source,
            alias,
            slot: _,
        } => {
            let alias_str = alias
                .as_ref()
                .map(|a| format!(" * as {a}"))
                .unwrap_or_else(|| " *".to_string());
            let _ = writeln!(out, "{ind}{YELLOW}export{R}{alias_str} from \"{source}\"");
        }
        HirStmt::ExportDefaultExpr { value, slot: _ } => {
            let _ = writeln!(out, "{ind}{YELLOW}export default{R} {}", dump_expr(value));
        }
        HirStmt::CloseUpvalues(targets) => {
            let _ = writeln!(
                out,
                "{ind}{DIM}close_upvalues({} targets){R}",
                targets.len()
            );
        }
        HirStmt::Dispose { target, is_await } => {
            let kw = if *is_await { "disposeAsync" } else { "dispose" };
            let _ = writeln!(out, "{ind}{DIM}{kw}(l{}){R}", target.0);
        }
    }
}

fn dump_expr(expr: &HirExpr) -> String {
    match expr {
        HirExpr::Int(n) => format!("{GREEN}{n}{R}"),
        HirExpr::Float(f) => format!("{GREEN}{f}{R}"),
        HirExpr::Bool(b) => format!("{GREEN}{b}{R}"),
        HirExpr::Char(c) => format!("{GREEN}'{c}'{R}"),
        HirExpr::Str(s) => format!("{GREEN}\"{s}\"{R}"),
        HirExpr::Decimal(d) => format!("{GREEN}{d}d{R}"),
        HirExpr::BigInt(n) => format!("{GREEN}{n}n{R}"),
        HirExpr::Regex { pattern, flags } => format!("{GREEN}/{pattern}/{flags}{R}"),
        HirExpr::Null => format!("{DIM}null{R}"),
        HirExpr::This => format!("{CYAN}this{R}"),
        HirExpr::Super => "super".to_string(),
        HirExpr::TaggedTemplate { tag, template } => {
            format!("{}[{}]", dump_expr(tag), dump_expr(template))
        }
        HirExpr::IntrinsicCall {
            object,
            args,
            wire_byte,
            ..
        } => {
            format!(
                "{MAGENTA}intrinsic:{wire_byte}{R}({}, [{}])",
                dump_expr(object),
                dump_exprs(args)
            )
        }
        HirExpr::NativeMethodCall {
            object,
            args,
            op_id,
            ..
        } => {
            format!(
                "{MAGENTA}nativeop:{op_id:#x}{R}({}, [{}])",
                dump_expr(object),
                dump_exprs(args)
            )
        }
        HirExpr::ModuleSlot { object, slot, .. } => {
            format!("{}.slot#{slot}", dump_expr(object))
        }

        HirExpr::Var(b) => dump_binding(b),

        HirExpr::NonNull(e) => format!("{}!", dump_expr(e)),
        HirExpr::TryOp(e) => format!("{}?", dump_expr(e)),
        HirExpr::Await(e) => format!("{YELLOW}await{R} {}", dump_expr(e)),
        HirExpr::Spawn(e) => format!("{YELLOW}spawn{R} {}", dump_expr(e)),
        HirExpr::Yield(e) => format!("{YELLOW}yield{R} {}", dump_expr(e)),
        HirExpr::Spread(e) => format!("...{}", dump_expr(e)),

        HirExpr::TypeTest { value, kind } => {
            let test = match kind {
                HirTypeTest::IsNull => "null".to_owned(),
                HirTypeTest::IsArray => "[]".to_owned(),
                HirTypeTest::TypeofEq(t) => format!("typeof=={GREEN}\"{t}\"{R}"),
                HirTypeTest::Instanceof(t) => format!("instanceof {t}"),
                HirTypeTest::AlwaysFalse => "false".to_owned(),
            };
            format!("({} is {test})", dump_expr(value))
        }

        HirExpr::Binary { op, lhs, rhs, ty } => {
            format!(
                "({} {DIM}{:?}{R}:{DIM}{}{R} {})",
                dump_expr(lhs),
                op,
                hir_ty(*ty),
                dump_expr(rhs)
            )
        }
        HirExpr::Unary { op, operand, ty } => {
            format!(
                "({DIM}{:?}{R}:{DIM}{}{R} {})",
                op,
                hir_ty(*ty),
                dump_expr(operand)
            )
        }
        HirExpr::Logical { op, lhs, rhs } => {
            let sym = match op {
                HirLogicalOp::And => "&&",
                HirLogicalOp::Or => "||",
                HirLogicalOp::Nullish => "??",
            };
            format!("({} {sym} {})", dump_expr(lhs), dump_expr(rhs))
        }
        HirExpr::Conditional { test, cons, alt } => {
            format!(
                "({} ? {} : {})",
                dump_expr(test),
                dump_expr(cons),
                dump_expr(alt)
            )
        }
        HirExpr::Update { target, op, prefix } => {
            let sym = match op {
                HirUpdateOp::Inc => "++",
                HirUpdateOp::Dec => "--",
            };
            let tgt = match target.as_ref() {
                HirAssignTarget::Var(b) => dump_binding(b),
                HirAssignTarget::Member { object, name } => {
                    format!("{}.{CYAN}{name}{R}", dump_expr(object))
                }
                HirAssignTarget::SetFixedField { object, slot } => {
                    format!("{}.slot#{slot}", dump_expr(object))
                }
                HirAssignTarget::Index { object, index, .. } => {
                    format!("{}[{}]", dump_expr(object), dump_expr(index))
                }
                HirAssignTarget::ModuleSlot { slot } => {
                    format!("slot#{slot}")
                }
                HirAssignTarget::SuperMember { name } => {
                    format!("super.{CYAN}{name}{R}")
                }
                HirAssignTarget::SuperIndex { index } => {
                    format!("super[{}]", dump_expr(index))
                }
            };
            if *prefix {
                format!("{sym}{tgt}")
            } else {
                format!("{tgt}{sym}")
            }
        }

        HirExpr::Call { callee, args, .. } => {
            format!(
                "{MAGENTA}call{R}({}, [{}])",
                dump_expr(callee),
                dump_exprs(args)
            )
        }
        HirExpr::SelfCall { args, .. } => {
            format!("{MAGENTA}self{R}([{}])", dump_exprs(args))
        }
        HirExpr::MethodCall {
            recv, name, args, ..
        } => {
            format!(
                "{MAGENTA}method{R}({}.{CYAN}{name}{R}, [{}])",
                dump_expr(recv),
                dump_exprs(args)
            )
        }
        HirExpr::SuperCall { args } => {
            format!("{MAGENTA}super{R}([{}])", dump_exprs(args))
        }
        HirExpr::SuperMethodCall { name, args } => {
            format!("{MAGENTA}super.{name}{R}([{}])", dump_exprs(args))
        }
        HirExpr::SuperMember { name } => {
            format!("{CYAN}super.{name}{R}")
        }
        HirExpr::ExtensionCall { func, recv, args } => {
            format!(
                "{MAGENTA}ext:{func}{R}({}, [{}])",
                dump_expr(recv),
                dump_exprs(args)
            )
        }

        HirExpr::Member { object, name, .. } => {
            format!("{}.{CYAN}{name}{R}", dump_expr(object))
        }
        HirExpr::GetFixedField { object, slot, .. } => {
            format!("{}.slot#{slot}", dump_expr(object))
        }
        HirExpr::MemberMaybe { object, name, .. } => {
            format!("{}?.{CYAN}{name}{R}", dump_expr(object))
        }
        HirExpr::Index { object, index, .. } => {
            format!("{}[{}]", dump_expr(object), dump_expr(index))
        }
        HirExpr::OptionalChain { object, property } => {
            let prop = match property {
                HirOptionalProperty::Member(n) => format!("?.{CYAN}{n}{R}"),
                HirOptionalProperty::Index(i) => format!("?.[{}]", dump_expr(i)),
                HirOptionalProperty::ModuleSlot(s) => format!("?.slot[{DIM}{s}{R}]"),
                HirOptionalProperty::Extension(n) => format!("?.ext:{n}"),
                HirOptionalProperty::Call(args) => format!("?.([{}])", dump_exprs(args)),
                HirOptionalProperty::MethodCall(n, args) => {
                    format!("?.{CYAN}{n}{R}([{}])", dump_exprs(args))
                }
                HirOptionalProperty::ExtensionCall(n, args) => {
                    format!("?.ext:{n}([{}])", dump_exprs(args))
                }
            };
            format!("{}{prop}", dump_expr(object))
        }

        HirExpr::Assign { target, value } => {
            let tgt = match target.as_ref() {
                HirAssignTarget::Var(b) => dump_binding(b),
                HirAssignTarget::Member { object, name } => {
                    format!("{}.{CYAN}{name}{R}", dump_expr(object))
                }
                HirAssignTarget::SetFixedField { object, slot } => {
                    format!("{}.slot#{slot}", dump_expr(object))
                }
                HirAssignTarget::Index { object, index, .. } => {
                    format!("{}[{}]", dump_expr(object), dump_expr(index))
                }
                HirAssignTarget::ModuleSlot { slot } => {
                    format!("slot#{slot}")
                }
                HirAssignTarget::SuperMember { name } => {
                    format!("super.{CYAN}{name}{R}")
                }
                HirAssignTarget::SuperIndex { index } => {
                    format!("super[{}]", dump_expr(index))
                }
            };
            format!("{tgt} = {}", dump_expr(value))
        }

        HirExpr::Sequence(es) => {
            format!("(seq: {})", dump_exprs(es))
        }
        HirExpr::Range {
            start,
            end,
            inclusive,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            format!("({}{op}{})", dump_expr(start), dump_expr(end))
        }
        HirExpr::Template(parts) => {
            let inner: Vec<String> = parts
                .iter()
                .map(|p| match p {
                    HirTemplatePart::Str(s) => format!("{GREEN}\"{s}\"{R}"),
                    HirTemplatePart::Expr(e) => format!("${{{}}}", dump_expr(e)),
                })
                .collect();
            format!("`{}`", inner.join(""))
        }

        HirExpr::Array(els) => {
            let items: Vec<String> = els
                .iter()
                .map(|el| match el {
                    HirArrayEl::Expr(e) => dump_expr(e),
                    HirArrayEl::Spread(e) => format!("...{}", dump_expr(e)),
                    HirArrayEl::Hole => format!("{DIM}hole{R}"),
                })
                .collect();
            format!("[{}]", items.join(", "))
        }
        HirExpr::Tuple(els) => {
            let items: Vec<String> = els
                .iter()
                .map(|el| match el {
                    HirArrayEl::Expr(e) => dump_expr(e),
                    HirArrayEl::Spread(e) => format!("...{}", dump_expr(e)),
                    HirArrayEl::Hole => format!("{DIM}hole{R}"),
                })
                .collect();
            format!("#[{}]", items.join(", "))
        }
        HirExpr::Object { properties } => {
            let props: Vec<String> = properties
                .iter()
                .map(|p| match p {
                    HirObjectProp::Property { key, value } => {
                        format!("{}: {}", dump_prop_key(key), dump_expr(value))
                    }
                    HirObjectProp::Method { key, .. } => {
                        format!("{DIM}method:{R} {}", dump_prop_key(key))
                    }
                    HirObjectProp::Spread(e) => format!("...{}", dump_expr(e)),
                })
                .collect();
            format!("{{{}}}", props.join(", "))
        }
        HirExpr::Record { properties } => {
            let props: Vec<String> = properties
                .iter()
                .map(|p| match p {
                    HirObjectProp::Property { key, value } => {
                        format!("{}: {}", dump_prop_key(key), dump_expr(value))
                    }
                    HirObjectProp::Method { key, .. } => {
                        format!("{DIM}method:{R} {}", dump_prop_key(key))
                    }
                    HirObjectProp::Spread(e) => format!("...{}", dump_expr(e)),
                })
                .collect();
            format!("#{{{}}}", props.join(", "))
        }
        HirExpr::ObjectRest { object, skip_keys } => {
            format!(
                "{{...rest({}, skip=[{}])}}",
                dump_expr(object),
                skip_keys
                    .iter()
                    .map(|k| format!("{CYAN}{k}{R}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }

        HirExpr::Closure { func, upvalues } => {
            format!(
                "{BLUE}closure{R}({}{DIM}, uvs={}{R})",
                func.name,
                upvalues.len()
            )
        }

        HirExpr::Class(cls) => {
            let super_str = cls
                .super_class
                .as_ref()
                .map(|s| format!(" extends {}", dump_expr(s)))
                .unwrap_or_default();
            format!(
                "{BLUE}class{R} {BOLD}{}{R}{super_str} \
                 {DIM}[fields={}, methods={}, static={}]{R}",
                cls.name,
                cls.fields.len(),
                cls.methods.len(),
                cls.static_methods.len(),
            )
        }
        HirExpr::Enum(en) => {
            format!(
                "{BLUE}enum{R} {BOLD}{}{R} {DIM}[{} variants]{R}",
                en.name,
                en.variants.len()
            )
        }
        HirExpr::Match { subject, cases } => {
            format!(
                "{YELLOW}match{R} {} {DIM}[{} cases]{R}",
                dump_expr(subject),
                cases.len()
            )
        }
    }
}

fn dump_binding(b: &HirBinding) -> String {
    match b {
        HirBinding::Param(i) => format!("{CYAN}p{i}{R}"),
        HirBinding::Local(id) => format!("{CYAN}l{}{R}", id.0),
        HirBinding::Global(n) => format!("{CYAN}g:{n}{R}"),
        HirBinding::Upvalue(i) => format!("{CYAN}uv{i}{R}"),
    }
}

fn dump_prop_key(k: &HirPropKey) -> String {
    match k {
        HirPropKey::Static(s) => format!("{CYAN}{s}{R}"),
        HirPropKey::Computed(e) => format!("[{}]", dump_expr(e)),
    }
}

fn dump_exprs(exprs: &[HirExpr]) -> String {
    exprs.iter().map(dump_expr).collect::<Vec<_>>().join(", ")
}

fn hir_ty(ty: HirType) -> &'static str {
    use varn_core::IntrinsicType;
    match ty {
        HirType::Int => IntrinsicType::Int.as_str(),
        HirType::Float => IntrinsicType::Float.as_str(),
        HirType::Bool => IntrinsicType::Bool.as_str(),
        HirType::Str => IntrinsicType::Str.as_str(),
        HirType::Ref => "ref",
        HirType::Dynamic => "dyn",
        // Nested TyIds need the module's TyTable to render; the dump shows
        // the shape only.
        HirType::Array(_) => "array",
        HirType::Map(_, _) => "map",
        HirType::Set(_) => "set",
        HirType::Class(_) => "class",
        HirType::Nullable(_) => "nullable",
    }
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}
