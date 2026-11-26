"""Direccionamiento multipunto con 9 bits: el noveno bit en 1 marca una DIRECCION y en 0 un DATO, a 9600 baudios.

Protocolo (el del modo de direccion del EUSART, aqui resuelto por software con read9):
  * Un caracter con el bit 8 en 1 es una direccion. Si es la de este nodo (DIRECCION), el nodo queda "atendido"; si es otra, deja de
    estar atendido y ignora los datos que sigan.
  * Un caracter con el bit 8 en 0 es un dato. Solo se atiende si el nodo esta atendido: 'A' (65) enciende RD0, 'B' (66) lo apaga y
    el nodo contesta con el dato recibido y el bit 8 en 0.

Conexion: RC6/TX y RC7/RX al bus (un RS-485 necesita su transceptor y controlar su direccion: este ejemplo no lo hace), tierra comun.

Politica ante lo inesperado:
  * Datos con el nodo sin atender: se ignoran sin contestar.
  * Un dato que no es 'A' ni 'B' se ignora.
  * Error de trama (FERR): el caracter se lee igual (FERR es del caracter que esta a punto de leerse); este ejemplo no lo mira.
  * Desbordamiento (OERR): se borra con clear_overrun(); los caracteres perdidos no se recuperan.
Hardware: NO probado, solo en simulador. Los 9 bits no estan verificados en Proteus todavia.
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK: Internal[4 * MHz]
DIRECCION: Const[uint16] = 0x05


def main() -> None:
    RD0.output()
    UART.start(baud=9600, bits=9)
    atendido: bool = False
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            c: uint16 = UART.recv9()
            if (c & 0x100) != 0:
                atendido = (c & 0xFF) == DIRECCION
            elif atendido:
                if c == 65:
                    RD0.on()
                    UART.write9(c)
                elif c == 66:
                    RD0.off()
                    UART.write9(c)
