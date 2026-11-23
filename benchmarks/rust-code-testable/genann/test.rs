fn main() {
	unsafe {
		// main_0 returns 1 when a test failed; pass it on as the exit code
		::std::process::exit(genann_rs::src::test::main_0(0, ::std::ptr::null_mut()));
	}
}
