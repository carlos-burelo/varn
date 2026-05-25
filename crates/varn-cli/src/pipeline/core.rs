use crate::error::CliError;

use varn_compiler::FunctionProto;

type PipelineResult<T> = Result<T, CliError>;

static CORE_PROTOS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/core.bin"));

use std::cell::RefCell;

thread_local! {
    static CORE_PROTOS: RefCell<Option<Result<Vec<FunctionProto>, String>>> = RefCell::new(None);
}

pub fn core_protos_owned() -> PipelineResult<Vec<FunctionProto>> {
    let result = CORE_PROTOS.with(|c| {
        let mut guard = c.borrow_mut();
        if guard.is_none() {
            *guard = Some(
                postcard::from_bytes(CORE_PROTOS_BYTES)
                    .map_err(|e| format!("failed to deserialize embedded core protos: {e}")),
            );
        }
        guard.as_ref().unwrap().clone()
    });

    result.map_err(|e| CliError::fatal(e))
}
