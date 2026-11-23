use ::libc;
extern "C" {
    fn BrotliDefaultAllocFunc(
        opaque: *mut u8,
        size: size_t,
    ) -> *mut u8;
    fn BrotliDefaultFreeFunc(opaque: *mut u8, address: *mut u8);
    fn exit(_: libc::c_int) -> !;
}
pub type size_t = libc::c_ulong;
pub type brotli_alloc_func = Option::<
    unsafe extern "C" fn(*mut u8, size_t) -> *mut u8,
>;
pub type brotli_free_func = Option::<
    unsafe extern "C" fn(*mut u8, *mut u8) -> (),
>;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct MemoryManager {
    pub alloc_func: brotli_alloc_func,
    pub free_func: brotli_free_func,
    pub opaque: *mut u8,
}
#[no_mangle]
pub unsafe extern "C" fn BrotliInitMemoryManager(
    mut m: *mut MemoryManager,
    mut alloc_func: brotli_alloc_func,
    mut free_func: brotli_free_func,
    mut opaque: *mut u8,
) {
    if alloc_func.is_none() {
        let ref mut fresh0 = (*m).alloc_func;
        *fresh0 = Some(
            BrotliDefaultAllocFunc
                as unsafe extern "C" fn(*mut u8, size_t) -> *mut u8,
        );
        let ref mut fresh1 = (*m).free_func;
        *fresh1 = Some(
            BrotliDefaultFreeFunc
                as unsafe extern "C" fn(*mut u8, *mut u8) -> (),
        );
        let ref mut fresh2 = (*m).opaque;
        *fresh2 = 0 as *mut u8;
    } else {
        let ref mut fresh3 = (*m).alloc_func;
        *fresh3 = alloc_func;
        let ref mut fresh4 = (*m).free_func;
        *fresh4 = free_func;
        let ref mut fresh5 = (*m).opaque;
        *fresh5 = opaque;
    };
}
#[no_mangle]
pub unsafe extern "C" fn BrotliAllocate(
    mut m: *mut MemoryManager,
    mut n: size_t,
) -> *mut u8 {
    let mut result = ((*m).alloc_func)
        .expect("non-null function pointer")((*m).opaque, n);
    if result.is_null() {
        exit(1 as libc::c_int);
    }
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn BrotliFree(
    mut m: *mut MemoryManager,
    mut p: *mut u8,
) {
    ((*m).free_func).expect("non-null function pointer")((*m).opaque, p);
}
#[no_mangle]
pub unsafe extern "C" fn BrotliWipeOutMemoryManager(mut m: *mut MemoryManager) {}
