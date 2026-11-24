"""Eco por el puerto serie: devuelve cada byte recibido incrementado en uno, a 9600 baudios (8N1) por RC6 (TX) y RC7 (RX)."""
from pypic.device import PIC16F887
from pypic.periph.uart import UART

CLOCK_HZ: Const[u32] = 4_000_000   # a 4 MHz el compilador usa BRGH = 1 y SPBRG = 25: 9615 baudios (+0,16 %)


def main() -> None:
    UART.start(baud=9600)
    while True:
        dato: u8 = UART.read()         # espera RCIF y lee RCREG
        UART.write(dato + 1)           # espera TXIF y escribe TXREG
