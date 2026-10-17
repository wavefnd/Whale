.text
.global _start
_start:
    addi.d $a0, $zero, 42
    addi.d $a7, $zero, 93
    syscall 0
