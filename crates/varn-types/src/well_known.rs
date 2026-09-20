use std::sync::Arc;
use varn_core::well_known as core;

lazy_static::lazy_static! {
    pub static ref ERR_IS_varn_ERROR: Arc<str> = Arc::from(core::ERR_IS_varn_ERROR);
    pub static ref ERR_MESSAGE: Arc<str> = Arc::from(core::ERR_MESSAGE);
    pub static ref ERR_STACK: Arc<str> = Arc::from(core::ERR_STACK);
    pub static ref ERR_NAME: Arc<str> = Arc::from(core::ERR_NAME);
    pub static ref ERR_FN: Arc<str> = Arc::from(core::ERR_FN);
    pub static ref ERR_LINE: Arc<str> = Arc::from(core::ERR_LINE);

    pub static ref ITERATOR: Arc<str> = Arc::from(core::ITERATOR);
    pub static ref ASYNC_ITERATOR: Arc<str> = Arc::from(core::ASYNC_ITERATOR);

    pub static ref PROTO_NEW: Arc<str> = Arc::from(core::PROTO_NEW);
    pub static ref PROTO_CTOR: Arc<str> = Arc::from(core::PROTO_CTOR);
    pub static ref PROTO_TO_STRING: Arc<str> = Arc::from(core::PROTO_TO_STRING);
    pub static ref PROTO_VALUE_OF: Arc<str> = Arc::from(core::PROTO_VALUE_OF);
}










