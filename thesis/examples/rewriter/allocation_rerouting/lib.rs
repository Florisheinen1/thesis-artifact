#![allow(dead_code)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![feature(register_tool)]
#![register_tool(c2rust)]
#![feature(strict_provenance)]
#![feature(core_intrinsics)]
#![feature(raw_ref_op)]

extern crate libc;
pub mod src {
pub mod main;
} // mod src
