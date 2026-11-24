"""Parpadeo de un LED en RD0: 500 ms encendido y 500 ms apagado, exactos al ciclo de instruccion."""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.system.time import pause

CLOCK_HZ: Const[uint32] = 4_000_000


@init_device
def configurar() -> None:
    RD0.output()


def main() -> None:
    while True:
        RD0.toggle()
        pause(500)



