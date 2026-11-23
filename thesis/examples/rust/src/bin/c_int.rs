// C's `int`, simulated in Rust through the libc crate.
// C counterpart: c/c_int.c

fn main() {
	// THESIS-SNIPPET: c_int
	use ::libc;
	let x: libc::c_int = 7;
	// THESIS-SNIPPET-END: c_int
	assert_eq!(x, 7);
}
