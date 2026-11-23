// A stack array whose length is only known at runtime cannot be expressed in
// Rust, not even in unsafe Rust.
// C counterpart: c/variable_length_array.c
//
// EXPECTED-COMPILE-ERROR: E0435

fn process(n: usize) {
	let mut buf = [0; n];
	buf[0] = 42;
}

fn main() {}
