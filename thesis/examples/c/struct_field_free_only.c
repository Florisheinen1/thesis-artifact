// Like struct_field_byte_offset.c, but `arr` is only freed. Without any
// array-specific operation, Crown cannot infer that `arr` is an array at all.

#include <stdint.h>
#include <stdlib.h>

// THESIS-SNIPPET: struct_field_free_only
static void use_array_of_struct(void *my_struct) {
	uint8_t *arr = *(uint8_t **)((char *)my_struct + 8);
	free(arr);
}
// THESIS-SNIPPET-END: struct_field_free_only
