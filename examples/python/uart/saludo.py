"""Saludo periodico por el puerto serie: envia "Hola PyxPic" y un salto de linea cada segundo, a 9600 baudios 8N1.

Conexion: RC6/TX del PIC -> RX del adaptador USB-UART; tierra comun. Terminal a 9600 8N1.
Los bytes del mensaje van explicitos en la memoria de programa (Rom): el lenguaje todavia no tiene cadenas.
Entrada: ninguna. Este ejemplo solo transmite, asi que no hay datos inesperados, errores de trama ni desbordamiento que tratar.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.uart import UART
from pyxpic.system.time import pause

CLOCK: Internal
FREQUENCY: 4MHz

# "Hola PyxPic" + CR + LF
MESSAGE: Rom[uint8] = [72, 111, 108, 97, 32, 80, 121, 120, 80, 105, 99, 13, 10]


def main() -> None:
    UART.start(baud=9600)
    while True:
        for i in range(13):
            UART.write(MESSAGE[i])
        while UART.sending():          # espera a que salga el ultimo bit antes de dormir
            pass
        pause(1000)
