use ::libc;
extern "C" {
    fn malloc(_: libc::c_ulong) -> *mut libc::c_void;
    fn free(_: *mut libc::c_void);
}
pub type size_t = libc::c_ulong;
#[no_mangle]
pub unsafe extern "C" fn BrotliDefaultAllocFunc(
    mut opaque: *mut u8,
    mut size: size_t,
) -> *mut u8 {
    return malloc(size) as *mut u8;
}
#[no_mangle]
pub unsafe extern "C" fn BrotliDefaultFreeFunc(
    mut opaque: *mut u8,
    mut address: *mut u8,
) {
    free(address as *mut libc::c_void);
}
