"""Escaner I2C: prueba las direcciones 1 a 127 y envia por la UART (9600 8N1) las que responden ACK, en hexadecimal.

SCL es RC3 y SDA es RC4 (con pull-ups de 4,7 k a VDD). La dirección que responde es de 7 bits: la que se escribe en I2C.write es esa
dirección desplazada un bit a la izquierda (el bit 0 es R/W = 0, escritura).
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.i2c import I2C
from pyxpic.periph.uart import UART
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


def main() -> None:
    I2C.start(freq=100000)
    UART.start(baud=9600)
    while True:
        hallados: uint8 = 0
        for dir7 in range(1, 128):
            I2C.begin()
            if I2C.write(dir7 << 1):
                UART.write_text(f"0x{dir7:02X}\r\n")
                hallados += 1
            I2C.end()
        UART.write_text(f"{hallados} dispositivo(s)\r\n")
        pause(2000)
