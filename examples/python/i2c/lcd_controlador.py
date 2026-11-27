"""Pantalla LCD 16x2 con controlador I2C propio (tipo ST7032 o AiP31068, direccion 0x3E): escribe "Hola PyxPic" e "I2C 100 kHz".

Distinta de la mochila PCF8574 de lcd_i2c.py: esta pantalla no usa nibbles ni el pulso de E. Cada transaccion es START, direccion 0x3E con R/W en 0 (se escribe
0x7C), un byte de control y un dato: control 0x00 = el dato es una orden, control 0x40 = el dato es un caracter. Las ordenes son las del HD44780 en 8 bits.
En Proteus es el modelo JHD-2X16-I2C con Device Address = $7C (Row 1 = 80-8F, Row 2 = C0-CF). Si tu pantalla real es una ST7032, necesita ademas las ordenes de la
tabla extendida (contraste e interno de alimentacion): este ejemplo no las manda.

Conexion: RC3/SCL y RC4/SDA con pull-ups de 4,7 kohm a VDD. Entrada: ninguna. Verificado solo en el simulador de PyxPic (sin pruebas en Proteus ni en hardware).
"""
from pyxpic.device import PIC16F887
from pyxpic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC
from pyxpic.system.time import delay_us, pause

CLOCK: Internal
FREQUENCY: 4MHz

DIRECCION: uint8 = 0x7C        # 0x3E << 1
CONTROL_ORDEN: uint8 = 0x00
CONTROL_DATO: uint8 = 0x40

LINEA1: Rom[uint8] = [72, 111, 108, 97, 32, 80, 121, 120, 80, 105, 99]              # "Hola PyxPic"
LINEA2: Rom[uint8] = [73, 50, 67, 32, 49, 48, 48, 32, 107, 72, 122]           # "I2C 100 kHz"


def reposo() -> None:
    while (SSPCON2.read() & 0x1F) != 0:
        pass


def enviar_byte(dato: uint8) -> None:
    SSPBUF.write(dato)
    while SSPSTAT.R_W.value() != 0:
        pass


def escribir(control: uint8, dato: uint8) -> None:
    SSPCON2.SEN.set()
    reposo()
    enviar_byte(DIRECCION)
    enviar_byte(control)
    enviar_byte(dato)
    SSPCON2.PEN.set()
    reposo()
    delay_us(100)                         # una orden tarda 37 us


def main() -> None:
    TRISC.write(TRISC.read() | 0x18)
    SSPADD.write(9)                       # 100 kHz a 4 MHz
    SSPCON.write(0x28)
    pause(200)                            # la pantalla necesita unos 40 ms tras alimentarse
    escribir(CONTROL_ORDEN, 0x38)         # 8 bits, 2 lineas, 5x8
    escribir(CONTROL_ORDEN, 0x0C)         # pantalla encendida, sin cursor
    escribir(CONTROL_ORDEN, 0x01)         # borrar
    pause(2)                              # el borrado tarda 1,52 ms
    escribir(CONTROL_ORDEN, 0x06)         # el cursor avanza a la derecha
    escribir(CONTROL_ORDEN, 0x80)         # primera linea
    for i in range(11):
        escribir(CONTROL_DATO, LINEA1[i])
    escribir(CONTROL_ORDEN, 0xC0)         # segunda linea
    for i in range(11):
        escribir(CONTROL_DATO, LINEA2[i])
    while True:
        pass
