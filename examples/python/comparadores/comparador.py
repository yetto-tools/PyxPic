"""Comparador 1 contra CVREF: enciende RD0 cuando la tension en RA0 supera la mitad de VDD (un comparador a secas, sin histeresis)."""
from pypic.device import PIC16F887
from pypic.periph.comparator import CMP1, C12IN0, REF_CVREF
from pypic.periph.gpio import RD0
from pypic.periph.vref import CVREF

CLOCK: Internal[4 * MHz]


def main() -> None:
    RD0.output()
    CVREF.start(step=8)                                   # VDD/4 + 8/32 x VDD = VDD/2
    CMP1.start(negative=C12IN0, positive=REF_CVREF)       # C1OUT = 1 cuando CVREF > RA0
    while True:
        if CMP1.out():
            RD0.off()                                     # CVREF por encima de RA0: RA0 esta por debajo de VDD/2
        else:
            RD0.on()                                      # RA0 por encima de VDD/2
