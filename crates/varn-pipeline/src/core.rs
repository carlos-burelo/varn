use crate::Failure;
use varn_compiler::FunctionProto;

type PipelineResult<T> = Result<T, Failure>;

pub fn core_protos_owned() -> PipelineResult<Vec<FunctionProto>> {
    Ok(Vec::new())
}
