section .text
global _start
_start:
    ; Complex instruction sequence
    xor eax, eax
    mov ecx, 10
.loop:
    add eax, ecx
    loop .loop
    ret
