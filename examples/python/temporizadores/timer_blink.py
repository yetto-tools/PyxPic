"""Parpadeo de RD0 cada 500 ms con Timer2 en lugar de una espera bloqueante: 25 avisos de 20 ms."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.periph.timer import TMR2

CLOCK: Internal[4 * MHz]


@init_device
def configurar() -> None:
    RD0.output()


def main() -> None:
    cuenta: uint8 = 0
    TMR2.every_ms(20)
    while True:
        if TMR2.expired():
            TMR2.clear()
            cuenta += 1
            if cuenta == 25:
                cuenta = 0
                RD0.toggle()
