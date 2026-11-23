// A `while` loop, a construct both C and Rust support natively.
// C counterpart: c/while_loop.c

fn main() {
	let n = 10;
	let mut i = 0;
	// THESIS-SNIPPET: while_loop
	while i < n { i += 1; }
	// THESIS-SNIPPET-END: while_loop
	assert_eq!(i, n);
}
