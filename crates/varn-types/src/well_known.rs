use std::rc::Rc;
use varn_core::well_known as core;

lazy_static::lazy_static! {
    pub static ref ERR_IS_varn_ERROR: Rc<str> = Rc::from(core::ERR_IS_varn_ERROR);
    pub static ref ERR_MESSAGE: Rc<str> = Rc::from(core::ERR_MESSAGE);
    pub static ref ERR_STACK: Rc<str> = Rc::from(core::ERR_STACK);
    pub static ref ERR_NAME: Rc<str> = Rc::from(core::ERR_NAME);
    pub static ref ERR_FN: Rc<str> = Rc::from(core::ERR_FN);
    pub static ref ERR_LINE: Rc<str> = Rc::from(core::ERR_LINE);

    pub static ref ITERATOR: Rc<str> = Rc::from(core::ITERATOR);
    pub static ref ASYNC_ITERATOR: Rc<str> = Rc::from(core::ASYNC_ITERATOR);

    pub static ref PROTO_NEW: Rc<str> = Rc::from(core::PROTO_NEW);
    pub static ref PROTO_CTOR: Rc<str> = Rc::from(core::PROTO_CTOR);
    pub static ref PROTO_TO_STRING: Rc<str> = Rc::from(core::PROTO_TO_STRING);
    pub static ref PROTO_VALUE_OF: Rc<str> = Rc::from(core::PROTO_VALUE_OF);
}










