//!
//! Allocation functions that prepend a header before each allocation
//!
//! [ header | data .. ]
//! 		    ^
//! 			ptr
//!
//! header:
//! 	- contains `usize` amount of bytes allocated
//! 	- takes MAX_ALIGN bytes
//! data:
//! 	- the start of the requested allocation + size of allocation
//!
//! This means all allocations done through these functions will create
//! allocations that are always `max_align` larger than normal, which is
//! likely going to be 16 bytes (on 64-bit machines).
//!
//! These allocation calls can only be used on pointers directly retrieved
//! from these same allocation calls. Mixing with Rust's global allocator
//! with extern FFI allocation calls will result in undefined behavior.
//!
//!

// Comments starting with THESIS-SNIPPET mark code that is shown in the thesis
// (final_project.typ in the thesis-artifact repository, which reads it from
// this file). They are only markers and do not affect the code.

use std::convert::TryFrom;
use std::{
	alloc::{
		Layout, LayoutError, alloc_zeroed as global_alloc_zeroed, dealloc as global_dealloc,
		realloc as global_realloc,
	},
	ptr::{self, null_mut},
};

/// Maximum alignment of any primitive type on this platform
/// Needed to keep all allocated data guaranteed aligned
const MAX_ALIGN: usize = align_of::<libc::max_align_t>();

// Check that the header's usize fits in the alignment
// Just in case max_align happens to be larger than usize
const _: () = assert!(MAX_ALIGN >= size_of::<usize>());

/// Creates a layout, with both the header and the `user_size` requested
/// amount of bytes for the usable part of the allocation.
/// Returns both the layout and the offset to the start of the user's part of
/// the allocation in bytes
fn layout_with_header(user_size: usize) -> Result<(Layout, usize), LayoutError> {
	let header_layout = Layout::new::<usize>();
	let data_layout = Layout::from_size_align(user_size, MAX_ALIGN)?;

	header_layout.extend(data_layout)
}

/// Reads the amount of allocated bytes in the header of the given pointer
/// The pointer must be retrieved from header-based allocation calls
pub unsafe fn get_header<T>(ptr: *const T) -> *mut usize {
	unsafe { (ptr as *mut usize).byte_sub(MAX_ALIGN) }
}

/// Allocates similarly to C's malloc, while prepending a header containing
/// the amount of allocated data bytes
// THESIS-SNIPPET: header_malloc
pub unsafe extern "C" fn header_malloc<T>(size: T) -> *mut libc::c_void
where
	usize: TryFrom<T>,
	<usize as TryFrom<T>>::Error: std::fmt::Debug,
{
	let Ok(size) = usize::try_from(size) else {
		return ptr::null_mut();
	};

	let Ok((layout, offset)) = layout_with_header(size as usize) else {
		return ptr::null_mut();
	};

	// Allocate the entire block of memory
	let block = unsafe { global_alloc_zeroed(layout) };
	if block.is_null() {
		return ptr::null_mut();
	}
	unsafe {
		// Store the size at the beginning
		*(block as *mut usize) = size as usize;
		// And return the address of the first bit of data
		block.byte_add(offset) as *mut libc::c_void
	}
}
// THESIS-SNIPPET-END: header_malloc

/// Frees the given pointer similar to C's free.
/// The `ptr` must be retrieved from header-based allocation results
// THESIS-SNIPPET: header_free
pub unsafe extern "C" fn header_free(ptr: *mut libc::c_void) {
	if ptr.is_null() {
		return;
	}
	unsafe {
		let data_size = *get_header(ptr);
		let (layout, data_offset) = layout_with_header(data_size)
			.expect("Invalid value stored in header: Cannot construct its layout");

		let base_pointer = ptr.byte_sub(data_offset) as *mut u8;

		// THESIS-SNIPPET-HIDE: header_free debugging aid, left out of the thesis
		// Poison the DATA region only (not the header) with an obviously-wrong
		// bit pattern before actually deallocating it. Without this, a
		// use-after-free caused by a rewriter bug (freeing too early) can
		// silently read stale-but-still-plausible-looking data and pass a
		// test's own value checks -- undetectable without a tool like Miri.
		// The standard system-level way to get this for free, glibc's
		// MALLOC_PERTURB_ tunable, was tested directly against this
		// environment's glibc (small and large allocations, tcache enabled
		// and disabled) and confirmed to have no effect at all -- so this
		// runtime poisons on free itself instead, unconditionally, since we
		// control this allocator and correct code never reads freed memory
		// anyway.
		//
		// Deliberately NOT poisoning the header: a double-free's second call
		// still needs to read the *correct* original size here in the common
		// case, so it computes the right Layout and calls the real
		// global_dealloc a second time on a validly-sized, already-freed
		// block -- which is exactly what lets glibc's own double-free
		// detection (a much more specific, readable diagnostic than a
		// generic segfault) still catch it.
		//
		// Sanity-bound the poison write itself for the same reason: on some
		// double-frees, this header read is *also* already reading freed
		// memory (a genuine use-after-free of our own header, unrelated to
		// whether we poison it) -- confirmed directly: glibc's tcache writes
		// its own free-list bookkeeping into the first bytes of a freed
		// block, which is exactly where this header lives, so a double-free's
		// second `header_free` call can read tcache's internal bytes
		// reinterpreted as a byte count here, regardless of this poisoning
		// change. The original, unmodified code tolerated that silently,
		// because Rust's System allocator on Linux forwards straight to
		// libc's plain `free(ptr)`, which ignores the size argument entirely
		// -- so a garbage `data_size` there was harmless, and glibc's own
		// pointer-based double-free check still fired correctly. This
		// poisoning code is the first thing to actually *use* `data_size` as
		// a raw byte count (for `write_bytes`), so an unbounded garbage value
		// here would segfault immediately, before ever reaching
		// global_dealloc -- trading glibc's specific "double free detected"
		// message for an uninformative crash. Skip poisoning (but still
		// proceed to the real free below) whenever the size looks implausible
		// for this test suite, so the original detection path is preserved.
		const MAX_PLAUSIBLE_SIZE: usize = 1 << 20; // 1 MiB; every real allocation in this suite is tiny
		if data_size <= MAX_PLAUSIBLE_SIZE {
			ptr::write_bytes(ptr as *mut u8, 0xAA, data_size);
		}
		// THESIS-SNIPPET-HIDE-END

		global_dealloc(base_pointer, layout);
	}
}
// THESIS-SNIPPET-END: header_free

/// Allocates similarly to C's calloc, while prepending a header containing
/// the amount of allocated data bytes
pub unsafe extern "C" fn header_calloc<T, U>(num: T, size: U) -> *mut libc::c_void
where
	usize: TryFrom<T>,
	<usize as TryFrom<T>>::Error: std::fmt::Debug,
	usize: TryFrom<U>,
	<usize as TryFrom<U>>::Error: std::fmt::Debug,
{
	let Ok(num) = usize::try_from(num) else {
		// Allocation size does not fit in usize
		return ptr::null_mut();
	};
	let Ok(size) = usize::try_from(size) else {
		// Allocation size does not fit in usize
		return ptr::null_mut();
	};

	let Some(data_bytes) = num.checked_mul(size) else {
		return ptr::null_mut();
	};
	let Ok((layout, offset)) = layout_with_header(data_bytes) else {
		return ptr::null_mut();
	};
	let block = unsafe { global_alloc_zeroed(layout) };
	if block.is_null() {
		return ptr::null_mut();
	}

	unsafe {
		*(block as *mut usize) = data_bytes;
		block.byte_add(offset) as *mut libc::c_void
	}
}

/// Reallocates similarly to C's realloc, while prepending a header containing
/// the amount of allocated data bytes
pub unsafe extern "C" fn header_realloc<T>(
	old_ptr: *mut libc::c_void,
	new_size: T,
) -> *mut libc::c_void
where
	usize: TryFrom<T>,
	<usize as TryFrom<T>>::Error: std::fmt::Debug,
{
	if old_ptr.is_null() {
		unsafe { return header_malloc(new_size) };
	}
	let Ok(new_size) = usize::try_from(new_size) else {
		// Allocation size does not fit in usize
		return null_mut();
	};
	if new_size == 0 {
		unsafe {
			header_free(old_ptr);
		}
		return ptr::null_mut();
	}
	unsafe {
		let data_size = *get_header(old_ptr);

		let Ok((old_layout, old_data_offset)) = layout_with_header(data_size) else {
			return ptr::null_mut();
		};
		let Ok((new_layout, new_data_offset)) = layout_with_header(new_size) else {
			return ptr::null_mut();
		};

		let old_base_pointer = old_ptr.byte_sub(old_data_offset) as *mut u8;
		let new_base_pointer = global_realloc(old_base_pointer, old_layout, new_layout.size());

		if new_base_pointer.is_null() {
			return ptr::null_mut();
		}

		let new_pointer = new_base_pointer.byte_add(new_data_offset) as *mut libc::c_void;

		// Also zero the new bytes
		if new_size > data_size {
			ptr::write_bytes(
				(new_pointer as *mut u8).add(data_size),
				0,
				new_size - data_size,
			);
		}

		*get_header(new_pointer) = new_size;

		new_pointer
	}
}

/// Duplicates a string similarly to C's strdup, while prepending a header
/// containing the amount of allocated data bytes.
/// libc's own strdup allocates with C's malloc, so its result cannot be
/// freed with `header_free`
pub unsafe extern "C" fn header_strdup(s: *const libc::c_char) -> *mut libc::c_char {
	unsafe {
		let len = libc::strlen(s) as usize;
		let new = header_malloc::<usize>(len + 1) as *mut libc::c_char;
		if new.is_null() {
			return ptr::null_mut();
		}
		// The terminating null byte is already there, as header_malloc zeroes
		ptr::copy_nonoverlapping(s, new, len);
		new
	}
}

/// Duplicates at most `n` bytes of a string similarly to C's strndup, while
/// prepending a header containing the amount of allocated data bytes
pub unsafe extern "C" fn header_strndup<T>(s: *const libc::c_char, n: T) -> *mut libc::c_char
where
	usize: TryFrom<T>,
	<usize as TryFrom<T>>::Error: std::fmt::Debug,
{
	let Ok(n) = usize::try_from(n) else {
		// String length does not fit in usize
		return ptr::null_mut();
	};
	unsafe {
		let len = libc::strnlen(s, n as libc::size_t) as usize;
		let new = header_malloc::<usize>(len + 1) as *mut libc::c_char;
		if new.is_null() {
			return ptr::null_mut();
		}
		// The terminating null byte is already there, as header_malloc zeroes
		ptr::copy_nonoverlapping(s, new, len);
		new
	}
}
