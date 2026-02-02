start:
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, 13
    syscall

    push rax
    pop rbx
    add rbx, 10
    sub rbx, 5
    xor rcx, rcx
    cmp rbx, 15
    
    call some_func
    jmp end

some_func:
    nop
    ret

end:
    mov rax, 60
    xor rdi, rdi
    syscall

msg:
    db "Hello, Whale!", 10
