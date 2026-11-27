"""Lectura I2C: las ocho entradas de un PCF8574 se muestran en PORTD.

Conexion: SCL = RC3 y SDA = RC4 con resistencias de 4,7 k a VDD; PCF8574 con A0, A1 y A2 a tierra (direccion 0x20);
interruptores de P0..P7 a tierra; LED con resistencia en RD0..RD7. Un LED encendido es una entrada en alto (interruptor abierto).
Si el PCF8574 no responde, PORTD conserva el ultimo valor.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.i2c import I2C
from pyxpic.regs import PORTD, TRISD
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz


def main() -> None:
    TRISD.write(0)
    PORTD.write(0)
    I2C.start(freq=100000)
    I2C.begin()
    if I2C.write(0x40):          # 0x20 en escritura
        I2C.write(0xFF)          # los pines en alto para poder leerlos
    I2C.end()
    while True:
        I2C.begin()
        if I2C.write(0x41):      # 0x20 en lectura
            PORTD.write(I2C.read(ack=False))
        I2C.end()
        pause(50)
