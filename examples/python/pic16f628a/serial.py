"""Puerto serie del PIC16F628A a 9600 baudios 8N1: saluda al arrancar, devuelve cada caracter y controla el LED de RB0
('1' lo enciende, '0' lo apaga). Usa el oscilador interno de 4 MHz.

Conexion (PDIP-18): RB2/TX (8) -> RX del adaptador USB-UART; RB1/RX (7) <- TX del adaptador; tierra comun (5).
RB0 (6) -> resistencia de 330 ohm -> LED -> tierra. Niveles TTL de 5 V: no conectar un RS-232 directo.
"""
from pyxpic.device import PIC16F628A
from pyxpic.periph.gpio import RB0
from pyxpic.periph.uart import UART

CLOCK: Internal
FREQUENCY: 4MHz


@init_device
def configurar() -> None:
    RB0.output()
    RB0.off()
    UART.start(baud=9600)


def main() -> None:
    UART.write_text("Hola desde el PIC16F628A\r\n")
    while True:
        if UART.overrun():
            UART.clear_overrun()
        dato: uint8 = UART.read()
        if dato == 49:      # '1'
            RB0.on()
        if dato == 48:      # '0'
            RB0.off()
        UART.write(dato)
