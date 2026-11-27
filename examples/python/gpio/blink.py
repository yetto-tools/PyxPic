"""Parpadeo de un LED en RD0: 500 ms encendido y 500 ms apagado, exactos al ciclo de instruccion."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


@init_device
def configurar() -> None:
    RD0.output()


def main() -> None:
    while True:
        RD0.toggle()
        pause(500)



