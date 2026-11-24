from pypic.device import PIC16F887
from pypic import uint8, uint16, uint32, Const, interrupt
from pypic.periph.uart import UART
from pypic.system.time import pause

CLOCK_HZ: Const[uint32] = 4_000_000
LIMIT: Const[uint8] = 200

ticks: uint16 = 0


def add_sat(a: uint8, b: uint8) -> uint8:
    total: uint16 = uint16(a) + uint16(b)

    if total > 255:
        return 255

    return uint8(total)


@interrupt
def isr() -> None:
    global ticks
    ticks += 1


def main() -> None:
    UART.start(baud=9600)

    UART.write_text("Iniciando PyPIC\r\n")

    level: uint8 = 0

    for i in range(10):
        level = add_sat(level, i)
        UART.write_text(f"Iteracion: {i:02} Level: {level:03}\r\n")
        pause(100)

    UART.write_text("Incrementando nivel...\r\n")

    while level != LIMIT:
        level += 1

    UART.write_text(f"Resultado: {level:03}\r\n")
    UART.write_text(f"Ticks: {ticks:05}\r\n")

    # Prueba de saturacion
    result: uint8 = add_sat(200, 100)

    UART.write_text(f"Saturacion: {result:03}\r\n")
    UART.write_text("Programa finalizado\r\n")

    while True:
        pause(1000)