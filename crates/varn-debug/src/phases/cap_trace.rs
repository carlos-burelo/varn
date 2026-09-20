//! `-p caps` (aliases `cap`, `cap-trace`): required capabilities
//! (DEBUG_PLAN §4.9).

use std::io::Write;

use varn_types::FunctionProto;

use crate::fmt::Format;
use crate::render::{basename, BOLD, CYAN, DIM, RESET};
use crate::report::Report;

pub fn collect(proto: &FunctionProto) -> Report {
    Report::Rows(
        proto
            .required_caps
            .iter()
            .map(|cap| vec![cap.to_string()])
            .collect(),
    )
}

pub fn render(rep: &Report, filename: &str, fmt: Format, w: &mut dyn Write) -> std::io::Result<()> {
    let Report::Rows(rows) = rep else {
        return Ok(());
    };

    match fmt {
        Format::Plain => {
            write!(
                w,
                "\n{BOLD}Capability Trace{RESET}  {DIM}{filename}{RESET}\n"
            )?;
            if rows.is_empty() {
                write!(w, "  {DIM}(no capabilities required){RESET}\n")?;
            } else {
                write!(w, "  Required capabilities:\n")?;
                for r in rows {
                    write!(w, "    {CYAN}@cap{RESET}({BOLD}\"{}\"{RESET})\n", r[0])?;
                }
            }
        }
        Format::Text => {
            writeln!(w, "# caps {}", basename(filename))?;
            for r in rows {
                writeln!(w, "{}", r[0])?;
            }
        }
    }
    Ok(())
}

pub fn debug_cap_trace(proto: &FunctionProto, filename: &str) {
    let rep = collect(proto);
    let _ = render(&rep, filename, Format::Plain, &mut std::io::stderr());
}
