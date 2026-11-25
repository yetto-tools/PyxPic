"""Saludo periodico con UART.write_text: el mismo mensaje de saludo.py, sin escribir los bytes a mano, a 9600 baudios 8N1.

Conexion: RC6/TX del PIC -> RX del adaptador USB-UART; tierra comun. Terminal a 9600 8N1.
El texto es ASCII constante: el compilador lo guarda en la memoria de programa (no reserva ningun bufer en RAM) y write_text lo
envia byte a byte con UART.write, esperando TXIF entre cada uno. Los escapes validos son \\n \\r \\t \\0 \\\\ \\" y \\'.
Entrada: ninguna, asi que no hay datos inesperados, errores de trama ni desbordamiento que tratar.
Hardware: NO probado, solo en simulador (uart_text.rs).
"""
from pypic.device import PIC16F887
from pypic.periph.uart import UART
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]


def main() -> None:
    UART.start(baud=9600)
    while True:
        UART.write_text("Hola PyPIC\r\n")
        while UART.sending():          # espera a que salga el ultimo bit antes de dormir
            pass
        pause(1000)
