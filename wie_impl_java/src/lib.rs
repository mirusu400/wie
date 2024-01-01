#![no_std]
#![allow(unknown_lints)]
#![allow(clippy::needless_pass_by_ref_mut)]
extern crate alloc;

mod base;
mod handle;
pub mod r#impl;
mod method;

pub use self::base::{
    get_class_proto, JavaClassProto, JavaContext, JavaError, JavaFieldAccessFlag, JavaFieldProto, JavaMethodBody, JavaMethodFlag, JavaMethodProto,
    JavaResult,
};
