use ::libc;
extern "C" {
	fn malloc(
		_: libc::c_ulong,
	) -> *mut libc::c_void;
}

unsafe fn main() {
	let x = malloc(20);
}
