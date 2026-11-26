"""Comandos por la UART (9600 8N1): 1 enciende RD0, 0 lo apaga, t lo cambia, ? informa; otro byte vuelve como eco."""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK: Internal[4 * MHz]


def main() -> None:
    RD0.output()
    RD0.off()
    UART.start(baud=9600)
    UART.write_text("listo: 1 0 t ?\r\n")
    encendido: uint8 = 0
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            dato: uint8 = UART.recv()
            if dato == 49:      # '1'
                RD0.on()
                encendido = 1
                UART.write_text("LED encendido\r\n")
            elif dato == 48:    # '0'
                RD0.off()
                encendido = 0
                UART.write_text("LED apagado\r\n")
            elif dato == 116:   # 't'
                RD0.toggle()
                encendido ^= 1
                UART.write_text("LED cambiado\r\n")
            elif dato == 63:    # '?'
                UART.write_text(f"LED={encendido}\r\n")
            else:
                UART.write_text("eco: ")
                UART.write(dato)
                UART.write_text("\r\n")
