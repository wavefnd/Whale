section .text
global stack_test
stack_test:
    push rax
    push rbx
    pop rbx
    pop rax
    ret
