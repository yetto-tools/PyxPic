from pyxpic.device import PIC16F887
from pyxpic import uint8, uint16, Const, interrupt

LIMIT: Const[uint8] = 200
ticks: uint16 = 0


def add_sat(a: uint8, b: uint8) -> uint8:
    total: uint16 = uint16(a) + uint16(b)
    if total > 255:
        return 255
    return uint8(total)


@interrupt
def isr() -> None:
    global ticks
    ticks += 1


def main() -> None:
    level: uint8 = 0
    for i in range(10):
        level = add_sat(level, i)
    while level != LIMIT:
        level += 1
