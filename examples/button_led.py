"""Un pulsador en RB0 enciende el LED de RD0 (sin retardos: solo lee y escribe pines)."""
from pypic.device import PIC16F887
from pypic import Const, init_device, uint32
from pypic.periph.gpio import RB0, RD0

CLOCK: Internal[4 * MHz]


@init_device
def configurar() -> None:
    RD0.output()
    RB0.input()


def main() -> None:
    while True:
        if RB0.value() == 1:
            RD0.on()
        else:
            RD0.off()
