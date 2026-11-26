"""LED por comandos: 'A' (65) enciende RD0 y 'B' (66) lo apaga, a 9600 baudios 8N1.

Conexion: RC7/RX <- TX del adaptador USB-UART, tierra comun, un LED con resistencia en RD0. Terminal a 9600 8N1.

Politica ante lo inesperado:
  * Cualquier otro byte se ignora: el LED no cambia y no se contesta nada.
  * Error de trama (FERR): el byte se trata como cualquier otro (si es 'A' o 'B' actua).
  * Desbordamiento (OERR): se borra con clear_overrun() para seguir recibiendo; lo perdido no se recupera.
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK: Internal[4 * MHz]


def main() -> None:
    RD0.output()
    RD0.off()
    UART.start(baud=9600)
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            comando: uint8 = UART.recv()
            if comando == 65:
                RD0.on()
            elif comando == 66:
                RD0.off()
