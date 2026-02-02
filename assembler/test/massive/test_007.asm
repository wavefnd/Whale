section .text
global mem_test
mem_test:
    mov rax, [rbx]
    mov [rcx + 8], rdx
    mov r8, [r9 + r10 * 4 + 16]
    ret
