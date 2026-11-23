// A null check after the pointer is already dereferenced. Because the
// dereference would be undefined behavior for a null pointer, the compiler
// may assume the pointer is not null and remove the check.
// Compare with null_check_before_dereference.c.

#include <stddef.h>

// THESIS-SNIPPET: null_check_after_dereference
int foo(int* array) {
	int value = array[10];

	// Optimized away
	if (array == NULL) {
		return -1; // Return early
	}
	return value;
}
// THESIS-SNIPPET-END: null_check_after_dereference
