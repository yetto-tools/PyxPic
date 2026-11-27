"""Tabla de busqueda en la memoria de programa (RETLW): un seno de 16 pasos sin gastar RAM, como en ensamblador."""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0

# 16 entradas uint8 en la memoria de programa (Rom); se leen con SINE[i]; len(SINE) vale 16 al compilar
SINE: Rom[uint8] = [128, 176, 218, 245, 255, 245, 218, 176, 128, 79, 37, 10, 0, 10, 37, 79]

CLOCK: Internal
FREQUENCY: 4MHz
level: uint8 = 0


def main() -> None:
    global level
    RD0.output()
    step: uint8 = 0
    while True:
        level = SINE[step]          # 17 ciclos con cualquier indice
        if level > 127:
            RD0.on()
        else:
            RD0.off()
        step += 1
        if step == len(SINE):
            step = 0
