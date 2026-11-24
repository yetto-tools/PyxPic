"""Un pulsador en RB0 enciende el LED de RD0 (sin retardos: solo lee y escribe pines)."""
from pypic.gpio import RB0, RD0


def main() -> None:
    RD0.output()
    RB0.input()
    while True:
        if RB0.value() == 1:
            RD0.on()
        else:
            RD0.off()
