from pyxpic.device import PIC16F887
from pyxpic import uint8, asm_function

swapped: uint8 = 0


@asm_function
def swap_nibbles(x: uint8) -> uint8:
    """
    SWAPF x,W       ; los parametros y los globales se usan por su nombre
    RETURN          ; el resultado de 8 bits vuelve en W
    """


def main() -> None:
    global swapped
    swapped = swap_nibbles(0xA5)
