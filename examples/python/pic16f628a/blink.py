"""Parpadeo de un LED en RB0 del PIC16F628A: 500 ms encendido y 500 ms apagado, con el oscilador interno de 4 MHz.

Conexion (PDIP-18): VDD (14) a 5 V, VSS (5) a tierra, RB0 (6) -> resistencia de 330 ohm -> LED -> tierra.
"""
from pyxpic.device import PIC16F628A
from pyxpic.periph.gpio import RB0
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


@init_device
def configurar() -> None:
    RB0.output()


def main() -> None:
    while True:
        RB0.toggle()
        pause(500)
