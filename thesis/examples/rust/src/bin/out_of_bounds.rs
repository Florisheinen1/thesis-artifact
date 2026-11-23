// Reading an array at an index given on the command line. Run with 4, that
// is one element past the end, Rust's bounds check makes the program panic
// instead of reading outside the array.
// C counterpart: c/out_of_bounds.c
//
// CHECK-ARGS: 4
// CHECK-EXIT-CODE: 101

use std::env::args;

fn main() {
	// THESIS-SNIPPET: out_of_bounds
	let array = [0, 1, 2, 3];
	let index: usize = args()
		.nth(1).unwrap()
		.parse().unwrap();
	let last = array[index];
	// THESIS-SNIPPET-END: out_of_bounds
	println!("{last}");
}
