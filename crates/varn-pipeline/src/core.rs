use crate::PipelineError;
use varn_opt::FunctionProto;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn core_protos_owned() -> PipelineResult<Vec<FunctionProto>> {
    Ok(Vec::new())
}
