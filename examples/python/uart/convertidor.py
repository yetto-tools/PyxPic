"""Convertidor decimal <-> hexadecimal por el puerto serie, a 9600 baudios 8N1.

Escribes un numero y una linea ("31" o "0x1F" + Enter) y el PIC contesta con el mismo valor en las dos bases:
    31      ->  31 = 0x001F
    0x1F    ->  0x001F = 31
    65536   ->  ERR            (no cabe en 16 bits)
    12ab    ->  ERR            (no es decimal; para hexadecimal escribe el prefijo 0x)

Conexion: RC6/TX -> RX del adaptador, RC7/RX <- TX del adaptador, tierra comun. Terminal a 9600 8N1 que envie "\n" o "\r\n".
UART.read_line guarda la linea en el arreglo; parse_dec / parse_hex la convierten a un uint16 y anotan en `error` si no era un numero; el
resultado se escribe con una f-string ({v} decimal, {v:04X} hexadecimal de 4 digitos). El texto fijo vive en la memoria de programa.

Politica ante lo inesperado:
  * Linea vacia: se ignora, no contesta.
  * Texto que no es un numero, vacio de cifras, con espacios, con signo o mayor que 65535: contesta "ERR".
  * Linea de mas de 12 bytes: se guardan los primeros 12 (read_line descarta el resto) y se intenta convertir esos 12.
  * Desbordamiento del receptor (OERR): read_line lo borra y reinicia la linea; esa linea puede contestar "ERR" o un valor distinto.
Hardware: NO probado, solo en simulador (uart_examples.rs).
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.uart import UART

CLOCK: Internal[4 * MHz]

linea: uint8[12]


def main() -> None:
    UART.start(baud=9600)
    error: bool = False
    valor: uint16 = 0
    while True:
        n: uint8 = UART.read_line(linea)
        if n == 0:
            continue
        if n >= 2 and linea[0] == 48 and (linea[1] == 120 or linea[1] == 88):     # empieza por "0x" o "0X"
            valor = parse_hex(linea, n, error)
            if error:
                UART.write_text("ERR\r\n")
            else:
                UART.write_text(f"0x{valor:04X} = {valor}\r\n")
        else:
            valor = parse_dec(linea, n, error)
            if error:
                UART.write_text("ERR\r\n")
            else:
                UART.write_text(f"{valor} = 0x{valor:04X}\r\n")
