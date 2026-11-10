section .text
global call_test
extern external_func
call_test:
    call external_func
    ret
