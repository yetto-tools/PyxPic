from pypic.device import PIC16F887
from pypic import u8, u16, Const, interrupt

LIMIT: Const[u8] = 200
ticks: u16 = 0


def add_sat(a: u8, b: u8) -> u8:
    total: u16 = u16(a) + u16(b)
    if total > 255:
        return 255
    return u8(total)


@interrupt
def isr() -> None:
    global ticks
    ticks += 1


def main() -> None:
    level: u8 = 0
    for i in range(10):
        level = add_sat(level, i)
    while level != LIMIT:
        level += 1
