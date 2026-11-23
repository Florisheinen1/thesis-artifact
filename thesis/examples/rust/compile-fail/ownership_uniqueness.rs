// Every value has a single owner. Moving ownership invalidates both the old
// owner and the references made through it.
//
// EXPECTED-COMPILE-ERROR: E0382
// EXPECTED-COMPILE-ERROR: E0505

#![allow(unused_variables, path_statements)]

struct Obj {
	i: i32,
}

fn main() {
	// THESIS-SNIPPET: ownership_uniqueness
	let my_obj = Obj { i: 67 };
	// `my_obj` owns the data

	let obj_ref = &my_obj;
	// references `my_obj`

	let new_own = my_obj;
	// Ownership moved to `new_own`

	my_obj.i; // error
	obj_ref.i; // error
	// THESIS-SNIPPET-END: ownership_uniqueness
}
