"""Tabla de busqueda en la memoria de programa (RETLW): un seno de 16 pasos sin gastar RAM, como en ensamblador."""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0

# 16 entradas u8 en flash; se leen con T[i]; len(SINE) vale 16 al compilar
SINE: Final[Table[u8]] = [128, 176, 218, 245, 255, 245, 218, 176, 128, 79, 37, 10, 0, 10, 37, 79]

CLOCK_HZ: Final[u32] = 4_000_000
level: u8 = 0


def main() -> None:
    global level
    RD0.output()
    step: u8 = 0
    while True:
        level = SINE[step]          # 17 ciclos con cualquier indice
        if level > 127:
            RD0.on()
        else:
            RD0.off()
        step += 1
        if step == len(SINE):
            step = 0
