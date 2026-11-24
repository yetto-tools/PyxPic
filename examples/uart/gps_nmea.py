"""Reconocimiento basico de tramas NMEA de un GPS (por ejemplo "$GPGGA,...*47"), a 9600 baudios 8N1.

ALCANCE: solo reconoce el inicio '$', comprueba el checksum (XOR de lo que hay entre '$' y '*') y cuenta las tramas validas, las
invalidas y las de tipo GGA. NO es un analizador GPS: no interpreta posicion, hora ni calidad de la senal.

Conexion: RC7/RX <- TX del modulo GPS (revisa que sus niveles los tolere el PIC), tierra comun.
Los modulos habituales emiten a 9600 8N1; comprueba la velocidad del tuyo.

Politica ante lo inesperado:
  * Bytes fuera de una trama se ignoran. Un '$' en cualquier momento reinicia la trama (se resincroniza tras una trama truncada).
  * Checksum incorrecto o digito que no es hexadecimal en MAYUSCULAS: la trama cuenta como invalida.
  * Error de trama (FERR): el byte se lee igual; el checksum de esa trama probablemente falle y cuente como invalida.
  * Desbordamiento (OERR): se borra con clear_overrun() y la trama en curso fallara el checksum.
  * RD0 se enciende con cada trama GGA valida y se apaga con cada trama invalida.
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK_HZ: Const[u32] = 4_000_000

# "GPGGA": el tipo de trama que se cuenta aparte
TIPO_GGA: Rom[u8] = [71, 80, 71, 71, 65]

validas: u8 = 0
invalidas: u8 = 0
gga: u8 = 0


def valor_hex(c: u8) -> u8:
    if c >= 48 and c <= 57:
        return c - 48
    if c >= 65 and c <= 70:
        return c - 55
    return 255


def main() -> None:
    global validas, invalidas, gga
    RD0.output()
    UART.start(baud=9600)
    estado: u8 = 0          # 0 espera '$', 1 cuerpo, 2 primer digito del checksum, 3 segundo digito
    suma: u8 = 0
    largo: u8 = 0
    es_gga: bool = False
    alto: u8 = 0
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            c: u8 = UART.recv()
            if c == 36:
                estado = 1
                suma = 0
                largo = 0
                es_gga = True
            elif estado == 1:
                if c == 42:
                    estado = 2
                else:
                    suma = suma ^ c
                    if largo < 5:
                        if c != TIPO_GGA[largo]:
                            es_gga = False
                    if largo < 255:
                        largo += 1
            elif estado == 2:
                alto = valor_hex(c)
                if alto == 255:
                    invalidas += 1
                    estado = 0
                else:
                    estado = 3
            elif estado == 3:
                bajo: u8 = valor_hex(c)
                estado = 0
                if bajo != 255 and ((alto << 4) | bajo) == suma:
                    validas += 1
                    if es_gga and largo >= 5:
                        gga += 1
                        RD0.on()
                else:
                    invalidas += 1
                    RD0.off()
