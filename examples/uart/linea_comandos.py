"""Comandos por linea: "ON\n" enciende RD0, "OFF\n" lo apaga y cualquier otra cosa contesta "ERR n"; a 9600 baudios 8N1.

Conexion: RC6/TX -> RX del adaptador, RC7/RX <- TX del adaptador, tierra comun, un LED en RD0. Terminal a 9600 8N1 que envie "\n" o "\r\n".
UART.read_line(buf) bloquea hasta recibir el \n, guarda la linea (sin \r ni \n) en el arreglo y devuelve cuantos bytes guardo.
El texto se compara byte a byte contra lo esperado: el lenguaje todavia no compara arreglos ni cadenas.

Politica ante lo inesperado:
  * Linea vacia: se ignora, no contesta.
  * Linea mas larga que el arreglo (16): se guardan 16 y se descarta el resto; como no coincidira con "ON" ni "OFF" contesta "ERR 16".
  * Comando desconocido: contesta "ERR n" con n = los bytes guardados.
  * Error de trama (FERR) o desbordamiento (OERR): no se tratan aqui; con OERR el chip deja de recibir hasta clear_overrun() y read_line se
    quedaria esperando. Con una terminal humana a 9600 baudios no llega a pasar; para trafico denso, ver errores.py.
Hardware: NO probado, solo en simulador (uart_examples.rs).
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK: Internal[4 * MHz]

linea: uint8[16]


def main() -> None:
    RD0.output()
    UART.start(baud=9600)
    while True:
        n: uint8 = UART.read_line(linea)
        if n == 0:
            continue
        if n == 2 and linea[0] == 79 and linea[1] == 78:                       # "ON"
            RD0.on()
            UART.write_text("OK\r\n")
        elif n == 3 and linea[0] == 79 and linea[1] == 70 and linea[2] == 70:  # "OFF"
            RD0.off()
            UART.write_text("OK\r\n")
        else:
            UART.write_text(f"ERR {n}\r\n")
