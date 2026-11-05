use std::backtrace::{Backtrace, BacktraceStatus};
use varn_core::term::chalk::chalk;

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "no message".to_owned()
    }
}

pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map_or_else(|| "unknown location".to_owned(), ToString::to_string);
        eprintln!(
            "{}: {}\n  {} {place}\n{}: this is a bug in Varn, not in your program; report it with the program that triggered it (varn {})",
            chalk("error").red().bold(),
            chalk(format!(
                "internal compiler error: {}",
                payload_message(info.payload())
            ))
            .bold(),
            chalk("-->").dim(),
            chalk("note").dim(),
            env!("CARGO_PKG_VERSION"),
        );
        let backtrace = Backtrace::capture();
        match backtrace.status() {
            BacktraceStatus::Captured => eprintln!("{backtrace}"),
            BacktraceStatus::Disabled | BacktraceStatus::Unsupported => eprintln!(
                "{}: run with RUST_BACKTRACE=1 to see where it happened",
                chalk("note").dim()
            ),
            _ => {}
        }
    }));
}
