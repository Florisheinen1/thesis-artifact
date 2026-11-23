// Dropping the same Box twice. Ownership rules reject the second drop, as the
// value has already been moved into the first.
// C counterpart: c/double_free.c
//
// EXPECTED-COMPILE-ERROR: E0382

fn main() {
	// THESIS-SNIPPET: double_free
	let heap_value = Box::new(42);
	drop(heap_value);
	drop(heap_value); // compile error: use of moved value
	// THESIS-SNIPPET-END: double_free
}
