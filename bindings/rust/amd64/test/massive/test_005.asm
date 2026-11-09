section .text
global jump_test
jump_test:
    cmp rax, rbx
    je .label1
    jmp .label2
.label1:
    nop
.label2:
    ret
