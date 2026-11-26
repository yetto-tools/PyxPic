"""Lee un potenciometro en AN0 y lo envia por el puerto serie como texto cada 500 ms: "ADC: 0512 (0x0200)\r\n", a 9600 baudios 8N1.

Conexion: potenciometro entre VDD y VSS con el cursor en RA0/AN0 (ver docs/sdk-adc.md para las condiciones electricas),
RC6/TX del PIC -> RX del adaptador USB-UART; tierra comun. Terminal a 9600 8N1.
El valor es una variable que solo se conoce al ejecutar: el PIC lo convierte a texto con codigo nativo (restas sucesivas para
el decimal y una tabla para el hexadecimal), sin intérprete de plantillas. El texto fijo vive en la memoria de programa y no hay
bufer de texto en RAM. {valor:04} rellena con ceros a 4 cifras; {valor:04X} muestra los 4 digitos hexadecimales de un uint16.
Entrada: ninguna. Este ejemplo solo transmite, asi que no hay datos inesperados, errores de trama ni desbordamiento que tratar.
Hardware: NO probado, solo en simulador (uart_examples.rs).
"""
from pypic.device import PIC16F887
from pypic.periph.adc import ADC, AN0
from pypic.periph.uart import UART
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]


def main() -> None:
    ADC.start()
    UART.start(baud=9600)
    while True:
        valor: uint16 = ADC.read(AN0)
        UART.write_text(f"ADC: {valor:04} (0x{valor:04X})\r\n")
        while UART.sending():
            pass
        pause(500)
