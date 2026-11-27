"""Lee un potenciometro en AN0 (RA0) y enciende RD0 cuando pasa de la mitad de la escala."""
from pyxpic.device import PIC16F887
from pyxpic.periph.adc import ADC, AN0
from pyxpic.periph.gpio import RD0

CLOCK: Internal[4 * MHz]   # el reloj de conversion (FOSC/8 a 4 MHz: TAD = 2 us) lo elige el compilador


def main() -> None:
    RD0.output()
    ADC.start()                    # resultado de 10 bits a la derecha, referencias VDD y VSS (ver docs/sdk-adc.md: condiciones electricas)
    while True:
        valor: uint16 = ADC.read(AN0)     # 0..1023: canal, adquisicion (11,5 us por defecto), conversion y resultado
        if valor > 511:
            RD0.on()
        else:
            RD0.off()
