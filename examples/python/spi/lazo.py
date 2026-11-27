"""SPI en lazo: envia un contador por SDO y enciende RD0 si vuelve igual por SDI.

Conexion: un cable de RC5 (SDO) a RC4 (SDI); un LED con resistencia en RD0. Sin el cable el LED queda apagado.
Pines del MSSP: SCK = RC3, SDI = RC4, SDO = RC5.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.periph.spi import SPI
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


def main() -> None:
    RD0.output()
    SPI.start(freq=1000000, mode=0)
    dato: uint8 = 0
    while True:
        recibido: uint8 = SPI.transfer(dato)
        if recibido == dato:
            RD0.on()
        else:
            RD0.off()
        dato = dato + 1
        pause(100)
