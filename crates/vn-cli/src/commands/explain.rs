use crate::cli::ExplainArgs;
use crate::error::CliError;
use vn_core::term::chalk::chalk;
use vn_core::ErrorCode;

fn parse_number(text: &str) -> Option<u32> {
    let digits = text
        .strip_prefix("VN")
        .or_else(|| text.strip_prefix("vn"))
        .unwrap_or(text);
    digits.parse().ok()
}

pub fn execute(args: ExplainArgs) -> Result<(), CliError> {
    let Some(code) = parse_number(&args.code).and_then(ErrorCode::from_number) else {
        return Err(CliError::usage(format!(
            "`{}` is not a Varn diagnostic code; codes look like VN3001",
            args.code
        )));
    };
    let severity = match code.severity() {
        vn_core::DiagnosticKind::Error => "error",
        vn_core::DiagnosticKind::Warning => "warning",
        vn_core::DiagnosticKind::Hint => "hint",
    };
    println!(
        "{} {} ({severity}, {})\n\n{}",
        chalk(code.to_string()).bold(),
        code.name(),
        code.category(),
        code.summary()
    );
    Ok(())
}
