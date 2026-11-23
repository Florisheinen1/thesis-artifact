// The four kinds of named pointer sources: a struct field, a function
// parameter, a let binding and a function return value.

#![allow(dead_code, unused_variables)]

type T = i32;

// THESIS-SNIPPET: struct_field
struct MyObj {
	ptr: *mut T
}
// THESIS-SNIPPET-END: struct_field

// THESIS-SNIPPET: parameter
fn foo(ptr: *mut T) {}
// THESIS-SNIPPET-END: parameter

fn bar() {
	// THESIS-SNIPPET: let_binding
	let ptr = 0 as *mut T;
	// THESIS-SNIPPET-END: let_binding
}

// THESIS-SNIPPET: return_value
fn baz() -> *mut T { 0 as *mut T }
// THESIS-SNIPPET-END: return_value

fn main() {}
