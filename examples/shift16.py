"""Desplaza un valor de 16 bits una posicion a la izquierda con RLF encadenado por el acarreo, como en ensamblador."""
from pypic.device import PIC16F887
from pypic.bits import clear_carry, rol_c, carry, bit_set

lo: u8 = 0x81
hi: u8 = 0x40
overflow: bool = False
flags: u8 = 0


def main() -> None:
    global lo, hi, overflow, flags
    clear_carry()          # BCF STATUS,C
    lo = rol_c(lo)         # RLF lo,F
    hi = rol_c(hi)         # RLF hi,F
    overflow = carry()     # el bit que salio por la izquierda
    bit_set(flags, 0)      # BSF flags,0
    while True:
        pass
