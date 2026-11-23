// A C integer type, which Rust can only simulate through the libc crate.
// Rust counterpart: rust/src/bin/c_int.rs

int seven(void) {
	// THESIS-SNIPPET: c_int
	int x = 7;
	// THESIS-SNIPPET-END: c_int
	return x;
}
