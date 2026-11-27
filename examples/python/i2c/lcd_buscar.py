"""Busca en que direccion I2C esta una pantalla LCD con mochila PCF8574: recorre 0x08 a 0x77 y, a cada direccion que responde ACK, le manda el arranque
del HD44780 y el texto "DIR xx" con esa misma direccion en hexadecimal. La pantalla que escuche mostrara su propia direccion.

Ademas informa por la UART (9600 8N1, RC6/TX) de cada direccion que respondio: "ACK 27" y salto de linea. Es un escaner I2C sencillo (#77).
Se repite cada 3 segundos. Una mochila PCF8574 contesta ACK a su direccion; el modelo del LCD de Proteus o la pantalla real, con su disposicion de bits (P0 = RS,
P1 = RW, P2 = E, P3 = luz, P4 a P7 = D4 a D7, como lcd_i2c.py) mostrara el texto.
Conexion: RC3/SCL y RC4/SDA con pull-ups de 4,7 kohm a VDD; RC6/TX a un terminal serie si quieres la lista. Entrada: ninguna.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.uart import UART
from pyxpic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC
from pyxpic.system.time import delay_us, pause

CLOCK: Internal
FREQUENCY: 4MHz

BIT_RS: uint8 = 0x01
BIT_E: uint8 = 0x04
BIT_LUZ: uint8 = 0x08

HEX: Rom[uint8] = [48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 65, 66, 67, 68, 69, 70]     # "0123456789ABCDEF"
DIR: Rom[uint8] = [68, 73, 82, 32]                                                   # "DIR "
ACK: Rom[uint8] = [65, 67, 75, 32]                                                   # "ACK "


def reposo() -> None:
    while (SSPCON2.read() & 0x1F) != 0:
        pass


def enviar_byte(dato: uint8) -> None:
    SSPBUF.write(dato)
    while SSPSTAT.R_W.value() != 0:
        pass


def sondear(d: uint8) -> uint8:
    # direccion de 7 bits d; devuelve 1 si respondio ACK
    SSPCON2.SEN.set()
    reposo()
    enviar_byte(d << 1)
    nack: uint8 = SSPCON2.ACKSTAT.value()
    SSPCON2.PEN.set()
    reposo()
    return 1 - nack


def expansor(d: uint8, dato: uint8) -> None:
    SSPCON2.SEN.set()
    reposo()
    enviar_byte(d << 1)
    enviar_byte(dato)
    SSPCON2.PEN.set()
    reposo()


def nibble(d: uint8, n: uint8, rs: uint8) -> None:
    v: uint8 = (n << 4) | BIT_LUZ | rs
    expansor(d, v | BIT_E)
    expansor(d, v)


def enviar(d: uint8, b: uint8, rs: uint8) -> None:
    nibble(d, b >> 4, rs)
    nibble(d, b & 0x0F, rs)
    delay_us(100)


def pantalla(d: uint8) -> None:
    pause(50)
    nibble(d, 3, 0)
    pause(5)
    nibble(d, 3, 0)
    delay_us(150)
    nibble(d, 3, 0)
    nibble(d, 2, 0)
    enviar(d, 0x28, 0)
    enviar(d, 0x0C, 0)
    enviar(d, 0x01, 0)
    pause(2)
    enviar(d, 0x06, 0)
    enviar(d, 0x80, 0)
    for i in range(4):
        enviar(d, DIR[i], BIT_RS)
    enviar(d, HEX[d >> 4], BIT_RS)
    enviar(d, HEX[d & 0x0F], BIT_RS)


def informar(d: uint8) -> None:
    for i in range(4):
        UART.write(ACK[i])
    UART.write(HEX[d >> 4])
    UART.write(HEX[d & 0x0F])
    UART.write(13)
    UART.write(10)
    while UART.sending():
        pass


def main() -> None:
    UART.start(baud=9600)
    TRISC.write(TRISC.read() | 0x18)
    SSPADD.write(9)
    SSPCON.write(0x28)
    while True:
        for d in range(8, 120):
            if sondear(d) == 1:
                informar(d)
                pantalla(d)
        pause(3000)
