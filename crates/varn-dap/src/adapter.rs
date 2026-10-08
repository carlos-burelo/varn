use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::driver::{Driver, ResumeOutcome};
use crate::protocol::{Event, LaunchArgs, Request, Response, SetBreakpointsArgs};

pub fn run_stdio() {
    let stdin = std::io::stdin();
    let mut adapter = Adapter::new();
    let mut handle = stdin.lock();
    while let Some(msg) = read_message(&mut handle) {
        if !adapter.handle(&msg) {
            break;
        }
    }
}

struct Adapter {
    seq: i64,
    driver: Driver,
    entry_pending: bool,
    launched: bool,
}

impl Adapter {
    fn new() -> Self {
        Self {
            seq: 0,
            driver: Driver::new(),
            entry_pending: false,
            launched: false,
        }
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    fn respond(
        &mut self,
        req: &Request,
        success: bool,
        message: Option<String>,
        body: Option<Value>,
    ) {
        let seq = self.next_seq();
        let resp = Response {
            seq,
            r#type: "response",
            request_seq: req.seq,
            success,
            command: req.command.clone(),
            message,
            body,
        };
        write_message(&serde_json::to_string(&resp).unwrap_or_default());
    }

    fn ok(&mut self, req: &Request, body: Option<Value>) {
        self.respond(req, true, None, body);
    }

    fn fail(&mut self, req: &Request, message: String) {
        self.respond(req, false, Some(message), None);
    }

    fn event(&mut self, event: &'static str, body: Option<Value>) {
        let seq = self.next_seq();
        let ev = Event::new(seq, event, body);
        write_message(&serde_json::to_string(&ev).unwrap_or_default());
    }

    fn stopped(&mut self, reason: &'static str) {
        self.event("stopped", Some(json!({"reason": reason, "threadId": 1})));
    }

    fn handle(&mut self, raw: &str) -> bool {
        let req: Request = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(_) => return true,
        };
        match req.command.as_str() {
            "initialize" => {
                self.ok(
                    &req,
                    Some(json!({
                        "supportsConfigurationDoneRequest": true,
                        "supportsTerminateDebuggee": true,
                        "supportsRestartRequest": false,
                    })),
                );
                self.event("initialized", None);
            }
            "launch" => {
                let args: LaunchArgs = req
                    .arguments
                    .clone()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                match args.program {
                    Some(program) => match self.driver.launch(&program, args.stop_on_entry) {
                        Ok(entry) => {
                            self.entry_pending = entry;
                            self.launched = true;
                            self.ok(&req, None);
                        }
                        Err(e) => self.fail(&req, e),
                    },
                    None => self.fail(&req, "launch requires a program path".to_string()),
                }
            }
            "setBreakpoints" => {
                let args: SetBreakpointsArgs = match req
                    .arguments
                    .clone()
                    .and_then(|v| serde_json::from_value(v).ok())
                {
                    Some(a) => a,
                    None => {
                        self.fail(&req, "bad setBreakpoints arguments".to_string());
                        return true;
                    }
                };
                let path = args.source.path.or(args.source.name).unwrap_or_default();
                let lines: Vec<u32> = args
                    .breakpoints
                    .unwrap_or_default()
                    .iter()
                    .map(|b| b.line)
                    .collect();
                let verified = self.driver.set_breakpoints(&path, &lines);
                let body: Vec<Value> = lines
                    .iter()
                    .zip(verified)
                    .map(|(line, v)| json!({"verified": v, "line": line}))
                    .collect();
                self.ok(&req, Some(json!({"breakpoints": body})));
            }
            "configurationDone" => {
                self.ok(&req, None);
                self.resume_with_reason();
            }
            "threads" => {
                self.ok(&req, Some(json!({"threads": [{"id": 1, "name": "main"}]})));
            }
            "stackTrace" => {
                let frames = match self.driver.machine() {
                    Some(m) => varn_vm::debug::snapshot_frames(&m.ctx),
                    None => {
                        self.fail(&req, "not launched".to_string());
                        return true;
                    }
                };
                let list: Vec<Value> = frames
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        json!({
                            "id": i,
                            "name": f.fn_name,
                            "source": {"name": short_name(&f.file), "path": f.file},
                            "line": f.line,
                            "column": 1,
                        })
                    })
                    .collect();
                let total = list.len();
                self.ok(
                    &req,
                    Some(json!({"stackFrames": list, "totalFrames": total})),
                );
            }
            "scopes" => {
                let frame: i64 = req
                    .arguments
                    .clone()
                    .and_then(|v| v.get("frameId").and_then(|f| f.as_i64()))
                    .unwrap_or(0);
                self.ok(
                    &req,
                    Some(json!({"scopes": [{
                        "name": "Locals",
                        "variablesReference": 1000 + frame,
                        "expensive": false,
                    }]})),
                );
            }
            "variables" => {
                let var_ref: i64 = req
                    .arguments
                    .clone()
                    .and_then(|v| v.get("variablesReference").and_then(|f| f.as_i64()))
                    .unwrap_or(0);
                let frame = var_ref - 1000;
                let vars = match self.driver.machine() {
                    Some(m) if frame >= 0 => varn_vm::debug::snapshot_vars(&m.ctx, frame as usize),
                    _ => {
                        self.fail(&req, "bad variablesReference".to_string());
                        return true;
                    }
                };
                let list: Vec<Value> = vars
                    .iter()
                    .map(|v| {
                        json!({
                            "name": v.name,
                            "value": v.value,
                            "variablesReference": 0,
                        })
                    })
                    .collect();
                self.ok(&req, Some(json!({"variables": list})));
            }
            "continue" => {
                self.ok(&req, Some(json!({"allThreadsContinued": true})));
                self.entry_pending = false;
                self.resume_with_reason();
            }
            "next" => {
                self.ok(&req, None);
                self.entry_pending = false;
                self.step_with(varn_vm::debug::StepKind::Over, "step");
            }
            "stepIn" => {
                self.ok(&req, None);
                self.entry_pending = false;
                self.step_with(varn_vm::debug::StepKind::In, "step");
            }
            "stepOut" => {
                self.ok(&req, None);
                self.entry_pending = false;
                self.step_with(varn_vm::debug::StepKind::Out, "step");
            }
            "pause" => {
                self.fail(&req, "pause is not supported".to_string());
            }
            "disconnect" => {
                self.ok(&req, None);
                self.driver.disconnect();
                self.event("terminated", None);
                return false;
            }
            "terminate" => {
                self.ok(&req, None);
                self.driver.disconnect();
                self.event("terminated", None);
                return false;
            }
            _ => self.fail(&req, format!("unsupported command: {}", req.command)),
        }
        true
    }

    fn resume_with_reason(&mut self) {
        if !self.launched || self.driver.is_finished() {
            return;
        }
        let reason: &'static str = if self.entry_pending {
            self.entry_pending = false;
            "entry"
        } else {
            "breakpoint"
        };
        match self.driver.resume() {
            ResumeOutcome::Stopped => self.stopped(reason),
            ResumeOutcome::Terminated => self.event("terminated", None),
            ResumeOutcome::RuntimeError(msg) => {
                self.event("output", Some(json!({"category": "stderr", "output": msg})));
                self.event("terminated", None);
            }
            ResumeOutcome::Error(_) => {}
        }
    }

    fn step_with(&mut self, kind: varn_vm::debug::StepKind, reason: &'static str) {
        if !self.launched || self.driver.is_finished() {
            return;
        }
        match self.driver.step(kind) {
            ResumeOutcome::Stopped => self.stopped(reason),
            ResumeOutcome::Terminated => self.event("terminated", None),
            ResumeOutcome::RuntimeError(msg) => {
                self.event("output", Some(json!({"category": "stderr", "output": msg})));
                self.event("terminated", None);
            }
            ResumeOutcome::Error(_) => {}
        }
    }
}

fn short_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn read_message(input: &mut impl BufRead) -> Option<String> {
    let mut len: usize = 0;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            len = rest.trim().parse().ok()?;
        }
    }
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn write_message(text: &str) {
    let out = std::io::stdout();
    let mut handle = out.lock();
    let _ = write!(handle, "Content-Length: {}\r\n\r\n{text}", text.len());
    let _ = handle.flush();
}
