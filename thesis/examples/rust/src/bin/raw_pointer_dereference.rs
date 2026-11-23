// Dereferencing a raw pointer requires an `unsafe` block.

fn main() {
	// THESIS-SNIPPET: raw_pointer_dereference
	let num = 42;
	let ptr: *const i32 = &raw const num;
	// Mutable pointer is not allowed here
	// as `num` is not declared mutable

	let value: i32 = unsafe { *ptr };
	// THESIS-SNIPPET-END: raw_pointer_dereference
	assert_eq!(value, 42);
}
