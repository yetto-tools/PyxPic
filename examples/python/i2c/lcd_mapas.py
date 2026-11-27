"""Diagnostico de la mochila PCF8574 del LCD: prueba las dos disposiciones de bits mas comunes, una tras otra, y escribe "MAPA 1" o "MAPA 2".

  MAPA 1: P0 = RS, P1 = RW, P2 = E, P3 = luz, P4 a P7 = D4 a D7 (la de lcd_i2c.py)
  MAPA 2: P0 a P3 = D4 a D7, P4 = RS, P5 = RW, P6 = E, P7 = luz

Se repite cada 3 segundos. El texto que aparezca dice cual disposicion tiene tu mochila (o tu modelo de Proteus): cambia las constantes de lcd_i2c.py a esa.
Conexion: como lcd_i2c.py (RC3/SCL y RC4/SDA con pull-ups, direccion 0x27 = 0x4E). Entrada: ninguna.
"""
from pyxpic.device import PIC16F887
from pyxpic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC
from pyxpic.system.time import delay_us, pause

CLOCK: Internal[4 * MHz]

DIRECCION: uint8 = 0x4E        # 0x27 << 1

TEXTO: Rom[uint8] = [77, 65, 80, 65, 32]        # "MAPA "


def reposo() -> None:
    while (SSPCON2.read() & 0x1F) != 0:
        pass


def enviar_byte(dato: uint8) -> None:
    SSPBUF.write(dato)
    while SSPSTAT.R_W.value() != 0:
        pass


def expansor(dato: uint8) -> None:
    SSPCON2.SEN.set()
    reposo()
    enviar_byte(DIRECCION)
    enviar_byte(dato)
    SSPCON2.PEN.set()
    reposo()


# desp: posicion del nibble (4 = P7..P4, 0 = P3..P0); rs, e y luz: las mascaras de esos bits en el expansor
def nibble(n: uint8, rs: uint8, desp: uint8, e: uint8, luz: uint8) -> None:
    v: uint8 = luz | rs
    if desp == 4:
        v = v | (n << 4)
    else:
        v = v | n
    expansor(v | e)
    expansor(v)


def enviar(b: uint8, rs: uint8, desp: uint8, e: uint8, luz: uint8) -> None:
    nibble(b >> 4, rs, desp, e, luz)
    nibble(b & 0x0F, rs, desp, e, luz)
    delay_us(100)


def probar(numero: uint8, rs: uint8, desp: uint8, e: uint8, luz: uint8) -> None:
    pause(50)
    nibble(3, 0, desp, e, luz)
    pause(5)
    nibble(3, 0, desp, e, luz)
    delay_us(150)
    nibble(3, 0, desp, e, luz)
    nibble(2, 0, desp, e, luz)
    enviar(0x28, 0, desp, e, luz)
    enviar(0x0C, 0, desp, e, luz)
    enviar(0x01, 0, desp, e, luz)
    pause(2)
    enviar(0x06, 0, desp, e, luz)
    enviar(0x80, 0, desp, e, luz)
    for i in range(5):
        enviar(TEXTO[i], rs, desp, e, luz)
    enviar(48 + numero, rs, desp, e, luz)


def main() -> None:
    TRISC.write(TRISC.read() | 0x18)
    SSPADD.write(9)
    SSPCON.write(0x28)
    while True:
        probar(1, 0x01, 4, 0x04, 0x08)
        pause(3000)
        probar(2, 0x10, 0, 0x40, 0x80)
        pause(3000)
