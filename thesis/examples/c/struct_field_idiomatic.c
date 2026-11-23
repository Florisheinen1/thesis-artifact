// Accessing an array stored in a struct field the idiomatic way, explicit
// enough for Crown to see that `arr` is the struct's `y` field.
// Less idiomatic variants: struct_field_byte_offset.c, struct_field_free_only.c

#include <stddef.h>
#include <stdint.h>

// THESIS-SNIPPET: struct_field_idiomatic
typedef struct {
	int x;
	uint8_t *y;
	int z;
} MyStruct;

static void use_array_of_struct(MyStruct *my_struct) {
	uint8_t *arr = my_struct->y;
	arr[2] = 7;
}
// THESIS-SNIPPET-END: struct_field_idiomatic

// The other two variants hardcode the offset of `y` as 8 bytes: a 4-byte int
// followed by 4 bytes of padding. Compilation fails if that does not hold:
// the array size becomes -1. (C99 has no _Static_assert; that is C11.)
typedef char y_is_at_byte_offset_8[offsetof(MyStruct, y) == 8 ? 1 : -1];
