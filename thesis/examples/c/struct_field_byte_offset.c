// Accessing the same struct field as struct_field_idiomatic.c, but through a
// hardcoded byte offset (checked to be 8 in that file). Crown still sees that
// `arr` is an array, but no longer that it is the struct's `y` field.

#include <stdint.h>

// THESIS-SNIPPET: struct_field_byte_offset
static void use_array_of_struct(void *my_struct) {
	uint8_t *arr = *(uint8_t **)((char *)my_struct + 8);
	arr[2] = 7;
}
// THESIS-SNIPPET-END: struct_field_byte_offset
