"""Errores de recepcion: lee FERR ANTES del byte y se recupera de OERR, a 9600 baudios 8N1.

Conexion: RC6/TX -> RX del adaptador, RC7/RX <- TX del adaptador, tierra comun. Terminal a 9600 8N1.
El programa pausa 5 ms entre vueltas, como si estuviera ocupado en otra cosa: a 9600 baudios llegan unos 960 bytes por segundo,
asi que en 5 ms pueden llegar mas de los dos que caben en el FIFO y se produce un desbordamiento.

Politica (se contesta con un byte de estado):
  * Byte con error de trama (FERR = 1): se DESCARTA y se contesta '?' (63).
  * Desbordamiento (OERR = 1): se borra con clear_overrun(), se pierden los bytes que no cabian y se contesta 'O' (79).
  * Byte correcto: se devuelve tal cual.
FERR se consulta antes de leer porque pertenece al caracter que esta a punto de salir del FIFO (DS40001291H, 12.1.2.4).
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.uart import UART
from pyxpic.system.time import pause

CLOCK: Internal[4 * MHz]


def main() -> None:
    UART.start(baud=9600)
    while True:
        if UART.overrun():
            UART.clear_overrun()
            UART.write(79)
        if UART.rx_ready():
            con_error: bool = UART.framing_error()
            dato: uint8 = UART.recv()
            if con_error:
                UART.write(63)
            else:
                UART.write(dato)
        pause(5)
