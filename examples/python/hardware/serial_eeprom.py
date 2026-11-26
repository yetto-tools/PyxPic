"""Envia por la UART (9600 8N1, RC6/TX) el byte 0 de la EEPROM, una linea por segundo; RD0 cambia con cada linea."""
from pypic.device import PIC16F887
from pypic.periph.eeprom import EEPROM
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]


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
