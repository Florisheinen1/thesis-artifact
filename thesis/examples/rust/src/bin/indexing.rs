// Getting (a pointer to) the sixth element of an array: first through raw
// pointer offsets, as in C2Rust output, then through the bounds-checked
// methods of the rewriter's CBox type.

use cbox_runtime::cbox::CBox;

fn main() {
	let mut array = [0i32, 1, 2, 3, 4, 5, 6, 7];
	let ptr = array.as_mut_ptr();
	unsafe {
		// THESIS-SNIPPET: raw_offset
		let element_ptr = ptr.offset(5);
		let element = *ptr.offset(5);
		// THESIS-SNIPPET-END: raw_offset
		assert_eq!(*element_ptr, 5);
		assert_eq!(element, 5);
	}

	let box_ptr: CBox<[i32]> = CBox::from_malloc(8 * size_of::<i32>());
	unsafe {
		// THESIS-SNIPPET: cbox_get
		let element_ptr = box_ptr.get_ptr(5).unwrap();
		let element = box_ptr.get(5).unwrap();
		// THESIS-SNIPPET-END: cbox_get
		// Allocations are zeroed, so the element is 0.
		assert_eq!(*element_ptr, 0);
		assert_eq!(*element, 0);
	}
}
