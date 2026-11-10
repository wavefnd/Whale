/* Reference fixture for x86_64 SysV LP64. Run manually, not in cargo tests:
 * cc -std=c11 ir/tests/fixtures/amd64_layout.c -o /tmp/whale-abi-layout
 * /tmp/whale-abi-layout
 * Checked against GCC and Clang; no external tool is required by normal tests.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
struct Record { uint8_t a; uint64_t b; uint16_t c; };
struct Nested { _Bool a; struct Record b[3]; uint32_t c; };
struct Bytes { uint8_t a; uint8_t b[16]; uint8_t c; };
struct Wide { uint8_t a; __int128 b; uint8_t c; };
#define SHOW(T) printf("%zu %zu %zu %zu %zu\n", sizeof(T), _Alignof(T), offsetof(T, a), offsetof(T, b), offsetof(T, c))
int main(void) {
    SHOW(struct Record);
    SHOW(struct Nested);
    SHOW(struct Bytes);
    SHOW(struct Wide);
    printf("%zu %zu\n", sizeof(struct Record[3]), _Alignof(struct Record[3]));
    printf("%zu %zu\n", sizeof(_Float16), _Alignof(_Float16));
    return 0;
}
