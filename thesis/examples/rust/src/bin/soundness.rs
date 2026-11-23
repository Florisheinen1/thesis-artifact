// Two functions with the same precondition (`ptr` must not be null). Only
// the second marks itself `unsafe`, telling callers about that precondition;
// the first is an unsound safe abstraction.

#![allow(dead_code)]

// THESIS-SNIPPET: unsound
fn double_unsound(ptr: *mut i32) {
	let value = unsafe { *ptr };
	let double = value * 2;
	unsafe { *ptr = double };
}
// THESIS-SNIPPET-END: unsound

// THESIS-SNIPPET: sound
unsafe fn double_sound(ptr: *mut i32) {
	let value = unsafe { *ptr };
	let double = value * 2;
	unsafe { *ptr = double };
}
// THESIS-SNIPPET-END: sound

fn main() {}
