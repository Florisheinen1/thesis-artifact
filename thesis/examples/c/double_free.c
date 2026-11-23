// Freeing the same allocation twice: undefined behavior that C compiles
// (with a warning).
// Rust counterpart: rust/compile-fail/double_free.rs

#include <stdlib.h>

int main(void) {
	// THESIS-SNIPPET: double_free
	int *heap_value = malloc(1 * sizeof(int));
	free(heap_value);
	free(heap_value); // UB
	// THESIS-SNIPPET-END: double_free
	return 0;
}
