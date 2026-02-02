section .text
global _start
extern exit

_start:
    mov rax, 1          ; write syscall
    mov rdi, 1          ; stdout
    mov rsi, msg
    mov rdx, len
    syscall

    mov rax, 60         ; exit syscall
    xor rdi, rdi
    syscall

section .data
msg:
    db "Whale Toolchain: LLVM/GCC Competitor", 10
len:
    dq 37
