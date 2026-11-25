"""Eco: devuelve cada byte recibido SIN modificar (incluidos 0x00 y 0xFF), a 9600 baudios 8N1.

Conexion: RC6/TX -> RX del adaptador, RC7/RX <- TX del adaptador, tierra comun. Terminal a 9600 8N1.
(El ejemplo examples/uart_echo.py devuelve cada byte incrementado en uno: es otro ejemplo.)

Politica ante lo inesperado:
  * Cualquier valor de byte es un dato valido y se devuelve tal cual.
  * Error de trama (FERR): el byte se devuelve igual que se leyo; este ejemplo no mira FERR (errores.py si).
  * Desbordamiento (OERR): el chip deja de recibir hasta borrarlo, asi que se borra con clear_overrun() para no quedarse mudo;
    los bytes que llegaron mientras tanto se pierden.
"""
from pypic.device import PIC16F887
from pypic.periph.uart import UART

CLOCK: Internal[4 * MHz]


def main() -> None:
    UART.start(baud=9600)
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            UART.write(UART.recv())
