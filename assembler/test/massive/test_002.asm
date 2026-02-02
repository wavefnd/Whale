section .text
global add_func
add_func:
    add rdi, rsi
    mov rax, rdi
    ret
