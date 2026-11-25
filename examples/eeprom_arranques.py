"""Cuenta los arranques en la EEPROM (direccion 0) y parpadea RD0 tantas veces como arranques lleva: el contador sobrevive al apagado."""
from pypic.device import PIC16F887
from pypic.periph.eeprom import EEPROM
from pypic.periph.gpio import RD0
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]


def main() -> None:
    RD0.output()
    arranques: uint8 = EEPROM.read(0)      # una EEPROM virgen vale 0xFF
    if arranques == 255:
        arranques = 0
    arranques += 1
    EEPROM.write(0, arranques)             # espera hasta 6 ms (D122); el byte se borra y se escribe solo
    veces: uint8 = 0
    while veces < arranques:
        RD0.on()
        pause(200)
        RD0.off()
        pause(200)
        veces += 1
    while True:
        pass
