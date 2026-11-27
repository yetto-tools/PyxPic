"""Acceso directo a registros, al estilo ensamblador: PORTB con pull-ups débiles en RB0 y RB1 y lectura de un bit."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.regs import OPTION_REG, WPUB, PORTB

CLOCK: Internal
FREQUENCY: 4MHz


@init_device
def configurar() -> None:
    RD0.output()
    OPTION_REG.RBPU.clear()    # habilita los pull-ups débiles del puerto B
    WPUB.write(0b0000_0011)    # solo en RB0 y RB1


def main() -> None:
    while True:
        if PORTB.RB0.value() == 0:
            RD0.on()
        else:
            RD0.off()
