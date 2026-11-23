// Reading an array at an index given on the command line. Run with 4, that
// is one element past the end: undefined behavior, which compiles without
// warning and prints whatever happens to be in memory.
// Rust counterpart: rust/src/bin/out_of_bounds.rs

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
	if (argc < 2) {
		fprintf(stderr, "usage: %s INDEX\n", argv[0]);
		return 1;
	}
	// THESIS-SNIPPET: out_of_bounds
	int array[4] = {0, 1, 2, 3};
	int index = atoi(argv[1]);
	int last = array[index];
	// THESIS-SNIPPET-END: out_of_bounds
	printf("%d\n", last);
	return 0;
}
