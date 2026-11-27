"""Parpadeo de un LED en RB0: 500 ms encendido y 500 ms apagado, exactos al ciclo de instruccion."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RB0
from pyxpic.system.time import pause

CLOCK: Internal[4 * MHz]


@init_device
def configurar() -> None:
    RB0.output()


def main() -> None:
    while True:
        RB0.toggle()
        pause(500)



