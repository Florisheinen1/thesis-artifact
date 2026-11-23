// One mutable reference and any immutable references to the same data cannot
// be alive at the same time.
//
// EXPECTED-COMPILE-ERROR: E0502

#![allow(unused_variables, path_statements)]

struct Obj {
	i: i32,
}

fn main() {
	// THESIS-SNIPPET: mutable_exclusivity
	let mut my_obj = Obj { i: 67 };
	// `my_obj` owns the data

	let obj_ref_1 = &my_obj;
	let obj_ref_2 = &my_obj;
	// Multiple simultanous references

	let new_own = &mut my_obj;

	obj_ref_1.i; // error
	my_obj.i; // error
	// THESIS-SNIPPET-END: mutable_exclusivity
}
