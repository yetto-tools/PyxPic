"""Muestra el byte 0 de la EEPROM en RD0: tantos parpadeos como su valor, pausa de 2 s y repite. Solo lee."""
from pyxpic.device import PIC16F887
from pyxpic.periph.eeprom import EEPROM
from pyxpic.periph.gpio import RD0
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


def main() -> None:
    RD0.output()
    valor: uint8 = EEPROM.read(0)
    while True:
        veces: uint8 = 0
        while veces < valor:
            RD0.on()
            pause(300)
            RD0.off()
            pause(300)
            veces += 1
        pause(2000)
