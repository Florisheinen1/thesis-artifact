// A variable-length array on the stack, a construct absent in Rust.
// Rust counterpart: rust/compile-fail/variable_length_array.rs

// THESIS-SNIPPET: variable_length_array
void process(int n) {
	int buf[n];
	buf[0] = 42;
}
// THESIS-SNIPPET-END: variable_length_array
