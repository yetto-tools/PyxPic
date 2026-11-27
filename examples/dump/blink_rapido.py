"""Prueba de MCLRE=OFF: parpadeo rapido (100 ms) de un LED en RD0."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.system.time import pause

CLOCK: Internal[4 * MHz]


@init_device
def configurar() -> None:
    RD0.output()


def main() -> None:
    while True:
        RD0.toggle()
        pause(100)



