// A `while` loop, a construct both C and Rust support natively.
// Rust counterpart: rust/src/bin/while_loop.rs

int count_up(int n) {
	int i = 0;
	// THESIS-SNIPPET: while_loop
	while (i < n) { i++; }
	// THESIS-SNIPPET-END: while_loop
	return i;
}
