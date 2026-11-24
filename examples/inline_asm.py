from pypic.device import PIC16F887
from pypic import u8, asm_function

swapped: u8 = 0


@asm_function
def swap_nibbles(x: u8) -> u8:
    """
    SWAPF x,W       ; los parametros y los globales se usan por su nombre
    RETURN          ; el resultado de 8 bits vuelve en W
    """


def main() -> None:
    global swapped
    swapped = swap_nibbles(0xA5)
