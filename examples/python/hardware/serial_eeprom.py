"""Envia por la UART (9600 8N1, RC6/TX) el byte 0 de la EEPROM, una linea por segundo; RD0 cambia con cada linea."""
from pyxpic.device import PIC16F887
from pyxpic.periph.eeprom import EEPROM
from pyxpic.periph.gpio import RD0
from pyxpic.periph.uart import UART
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


def main() -> None:
    RD0.output()
    UART.start(baud=9600)
    n: uint8 = 0
    while True:
        valor: uint8 = EEPROM.read(0)
        UART.write_text(f"linea {n}  EEPROM[0]={valor}\r\n")
        RD0.toggle()
        n += 1
        pause(1000)
