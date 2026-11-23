// The same function as null_check_after_dereference.c, but with the null
// check before the dereference, so the compiler has to keep it.

#include <stddef.h>

// THESIS-SNIPPET: null_check_before_dereference
int foo(int* array) {
	if (array == NULL) {
		return -1; // Return early
	}

	int value = array[10];

	return value;
}
// THESIS-SNIPPET-END: null_check_before_dereference
