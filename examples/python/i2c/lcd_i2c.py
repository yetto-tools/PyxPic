"""Pantalla LCD 16x2 (HD44780) con la mochila I2C PCF8574: escribe "Hola PyxPic" en la primera linea y "I2C 100 kHz" en la segunda.

Conexion: RC3/SCL y RC4/SDA del PIC a SCL y SDA de la mochila, con resistencias de pull-up de 4,7 kohm a VDD en cada linea (el PIC no las tiene: muchas mochilas
ya traen las suyas); VDD y tierra comunes. La direccion de la mochila es 0x27 (PCF8574) o 0x3F (PCF8574A): se escribe ya desplazada (0x4E o 0x7E), que es lo que el
bus transmite con el bit R/W en 0. Cableado de la mochila mas comun: P0 = RS, P1 = RW, P2 = E, P3 = luz de fondo, P4 a P7 = D4 a D7; si la tuya es otra, cambia
las cuatro constantes de abajo.

El I2C usa el MSSP del PIC16F887 en modo maestro (docs/sdk-i2c.md) por medio de pyxpic.regs: todavia no hay un modulo pyxpic.periph.i2c. Cada byte hacia la mochila
es una transaccion completa (START, direccion, dato, STOP); el pulso de E se manda como dos bytes. Los tiempos son los del HD44780: E activo mas de 450 ns
(aqui, una transaccion de bus entera), 37 us por orden (se espera 100 us) y 1,52 ms tras borrar la pantalla (se espera 2 ms).
Entrada: ninguna.
"""
from pyxpic.device import PIC16F887
from pyxpic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC
from pyxpic.system.time import delay_us, pause

CLOCK: Internal[4 * MHz]

DIRECCION: uint8 = 0x40        # 0x20 << 1
BIT_RS:    uint8 = 0x01
BIT_E:     uint8 = 0x04
BIT_LUZ:   uint8 = 0x08

LINEA1: Rom[uint8] = [72, 111, 108, 97, 32, 80, 121, 120, 80, 105, 99]              # "Hola PyxPic"
LINEA2: Rom[uint8] = [73, 50, 67, 32, 49, 48, 48, 32, 107, 72, 122]           # "I2C 100 kHz"


def reposo() -> None:
    # el MSSP acepta una orden nueva solo sin SEN, RSEN, PEN, RCEN ni ACKEN en curso (SSPCON2 <4:0> en 0)
    while (SSPCON2.read() & 0x1F) != 0:
        pass


def i2c_iniciar() -> None:
    TRISC.write(TRISC.read() | 0x18)      # RC3 y RC4 como entradas: el MSSP las controla
    SSPADD.write(9)                       # Fosc/(4*(SSPADD+1)) = 4 MHz / 40 = 100 kHz
    SSPCON.write(0x28)                    # SSPEN = 1 y SSPM = 1000: I2C maestro


def enviar_byte(dato: uint8) -> None:
    SSPBUF.write(dato)
    while SSPSTAT.R_W.value() != 0:       # R/W = 1 mientras el byte (y su ACK) salen
        pass


def expansor(dato: uint8) -> None:
    SSPCON2.SEN.set()                     # START
    reposo()
    enviar_byte(DIRECCION)                # direccion + escritura
    enviar_byte(dato)
    SSPCON2.PEN.set()                     # STOP
    reposo()


def nibble(n: uint8, rs: uint8) -> None:
    # los 4 bits en P7..P4, la luz y RS abajo; E sube y baja para que el HD44780 los tome
    v: uint8 = (n << 4) | BIT_LUZ | rs
    expansor(v | BIT_E)
    expansor(v)


def enviar(b: uint8, rs: uint8) -> None:
    nibble(b >> 4, rs)
    nibble(b & 0x0F, rs)
    delay_us(100)                         # una orden tarda 37 us


def pantalla_iniciar() -> None:
    pause(200)                            # el HD44780 pide mas de 40 ms tras alimentarse; algunos modelos (Proteus JHD-2X16-I2C) tardan mas
    nibble(3, 0)                          # secuencia de arranque a 4 bits (datasheet del HD44780)
    pause(5)
    nibble(3, 0)
    delay_us(150)
    nibble(3, 0)
    nibble(2, 0)                          # pasa a 4 bits
    enviar(0x28, 0)                       # 4 bits, 2 lineas, 5x8
    enviar(0x0C, 0)                       # pantalla encendida, sin cursor
    enviar(0x01, 0)                       # borrar
    pause(2)                              # el borrado tarda 1,52 ms
    enviar(0x06, 0)                       # el cursor avanza a la derecha


def main() -> None:
    i2c_iniciar()
    pantalla_iniciar()
    enviar(0x80, 0)                       # primera linea
    for i in range(11):
        enviar(LINEA1[i], BIT_RS)
    enviar(0xC0, 0)                       # segunda linea
    for i in range(11):
        enviar(LINEA2[i], BIT_RS)
    while True:
        pass
