// Comments starting with THESIS-SNIPPET mark code that is shown in the thesis
// (final_project.typ in the thesis-artifact repository, which reads it from
// this file). They are only markers and do not affect the code.

use std::{marker::PhantomData, ptr::NonNull};

use crate::allocation::{get_header, header_calloc, header_free, header_malloc, header_realloc};

use std::convert::TryFrom;

/// A safe Rust alternative to the C raw pointer
/// Only drops its own allocation. Pointed-to elements
/// still need to be manually dropped
// THESIS-SNIPPET: cbox_struct
#[repr(transparent)]
pub struct CBox<T: ?Sized> {
	ptr: Option<NonNull<u8>>,
	_marker: PhantomData<*mut T>,
}
// THESIS-SNIPPET-END: cbox_struct

impl<T: ?Sized> CBox<T> {
	/// Creates a nullpointer-equivalent CBox
	pub fn null() -> Self {
		Self {
			ptr: None,
			_marker: PhantomData,
		}
	}
	/// Checks if this CBox represents a nullpointer
	pub fn is_null(&self) -> bool {
		self.ptr.is_none()
	}

	/// Similar to Option::take(), takes the contents of this box and returns it,
	/// while setting the contents of this box to None.
	pub fn take(&mut self) -> Self {
		Self {
			ptr: self.ptr.take(),
			_marker: PhantomData,
		}
	}

	/// Creates a new CBox based on the C malloc call
	pub fn from_malloc<U>(bytes: U) -> Self
	where
		usize: TryFrom<U>,
		<usize as TryFrom<U>>::Error: std::fmt::Debug,
	{
		let raw = unsafe { header_malloc(bytes) } as *mut u8;
		let ptr = NonNull::new(raw);

		// TODO: Assert correct `bytes` size based on T?

		Self {
			ptr,
			_marker: PhantomData,
		}
	}
	pub fn from_calloc<U, V>(element_count: U, element_size: V) -> Self
	where
		usize: TryFrom<U>,
		<usize as TryFrom<U>>::Error: std::fmt::Debug,
		usize: TryFrom<V>,
		<usize as TryFrom<V>>::Error: std::fmt::Debug,
	{
		let raw = unsafe { header_calloc(element_count, element_size) } as *mut u8;
		let ptr = NonNull::new(raw);

		Self {
			ptr,
			_marker: PhantomData,
		}
	}

	/// Performs a cast on the pointer.
	/// Has zero effects on the pointed-to data, only
	/// changes the perceived type of this container itself.
	/// Casted type is the type of the element this box points to.
	pub unsafe fn cast<U: ?Sized>(&self) -> &CBox<U> {
		unsafe { &*(self as *const CBox<T> as *const CBox<U>) }
	}
	/// Performs a cast on the pointer.
	/// Has zero effects on the pointed-to data, only
	/// changes the perceived type of this container itself.
	/// Casted type is the type of the element this box points to.
	pub unsafe fn cast_mut<U: ?Sized>(&mut self) -> &mut CBox<U> {
		unsafe { &mut *(self as *mut CBox<T> as *mut CBox<U>) }
	}
}

impl<T> CBox<T> {}
impl<T> CBox<[T]> {
	// THESIS-SNIPPET: get
	pub unsafe fn get(&self, index: usize) -> Option<&T> {
		let ptr = self.ptr?;
		let byte_len = *get_header(ptr.as_ptr());

		let len = byte_len / size_of::<T>();
		if index >= len {
			return None;
		}

		Some(&*(ptr.as_ptr() as *const T).add(index))
	}
	// THESIS-SNIPPET-END: get

	pub unsafe fn get_mut(&mut self, index: usize) -> Option<&mut T> {
		// TODO: Do not check for entire validity, only for the returned pointer
		let ptr = self.ptr?;
		let byte_len = *get_header(ptr.as_ptr());

		// TODO: Division or no division...
		let len = byte_len / size_of::<T>();
		if index >= len {
			return None;
		}

		Some(&mut *(ptr.as_ptr() as *mut T).add(index))
	}

	// Should an Option be returned or just the raw pointer? When is this ever 'None'?
	// THESIS-SNIPPET: get_ptr
	pub unsafe fn get_ptr(&self, index: usize) -> Option<*mut T> {
		let ptr = self.ptr?;
		let byte_len = *get_header(ptr.as_ptr());

		let byte_offset = index.checked_mul(size_of::<T>())?;
		// Specifically `>` instead of `>=`, because one pointer past the
		// allocation is technically still valid pointer arithmetic, as long
		// as the result is of course not dereferenced.
		if byte_offset > byte_len {
			return None;
		}
		Some((ptr.as_ptr() as *mut T).add(index))
	}
	// THESIS-SNIPPET-END: get_ptr

	pub unsafe fn from_raw(ptr: *const T) -> Self {
		Self {
			ptr: NonNull::new(ptr as *mut u8),
			_marker: PhantomData,
		}
	}

	/// Reallocates the current array
	/// Takes the new amount of bytes for the new allocation
	// THESIS-SNIPPET: from_realloc
	pub unsafe fn from_realloc<U>(&mut self, new_size: U) -> Self
	where
		U: Copy,
		usize: TryFrom<U>,
		<usize as TryFrom<U>>::Error: std::fmt::Debug,
	{
		// Check if new size is 0
		let is_zero = if let Ok(s) = usize::try_from(new_size) {
			s == 0
		} else {
			// If converting to usize failed, that is because of
			// large or negative numbers, which are by default
			// non-zero.
			false
		};

		// THESIS-SNIPPET-HIDE: from_realloc development notes, not shown in the thesis
		// TODO: Check if mutability still compiles at calling code
		// TODO: Assert correct new size
		// THESIS-SNIPPET-HIDE-END
		unsafe {
			let Some(raw) = self.ptr else {
				// Currently, self is nullptr
				return CBox::from_malloc(new_size);
			};
			let ptr = raw.as_ptr() as *mut libc::c_void;

			let raw_result = header_realloc(ptr, new_size);
			let result = NonNull::new(raw_result as *mut u8);

			if result.is_some() || is_zero {
				// Reallocation succeeded, previous pointer becomes invalid
				// Or, reallocated 0 (essentially free), and we need to set ptr to
				// None as well.
				self.ptr = None;
			}

			CBox {
				ptr: result,
				_marker: PhantomData,
			}
		}
	}
	// THESIS-SNIPPET-END: from_realloc

	// THESIS-SNIPPET: as_ptr_into_raw
	/// Returns the raw pointer of Self without consuming the ownership
	pub unsafe fn as_ptr(&self) -> *mut T {
		match &self.ptr {
			Some(ptr) => ptr.as_ptr() as *mut T,
			None => std::ptr::null_mut(),
		}
	}

	/// Extracts the raw pointer, leaving Self as a null CBox behind.
	/// Takes ownership of the pointer, without taking ownership of Self
	pub unsafe fn into_raw(&mut self) -> *mut T {
		match self.ptr.take() {
			Some(ptr) => ptr.as_ptr() as *mut T,
			None => std::ptr::null_mut(),
		}
	}
	// THESIS-SNIPPET-END: as_ptr_into_raw

	/// Returns a mutable pointer to the internal pointer of this object
	pub unsafe fn get_raw_ref_mut(&mut self) -> *mut *mut T {
		&mut self.ptr as *mut Option<NonNull<u8>> as *mut *mut T
	}
}

// THESIS-SNIPPET: drop
impl<T: ?Sized> Drop for CBox<T> {
	fn drop(&mut self) {
		unsafe {
			if let Some(ptr) = self.ptr {
				let ptr: *mut libc::c_void = ptr.as_ptr() as *mut libc::c_void;
				header_free(ptr);
			}
		}
	}
}
// THESIS-SNIPPET-END: drop
