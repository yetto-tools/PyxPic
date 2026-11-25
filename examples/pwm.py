"""Regula el brillo de un LED en RC2 (PWM1) con un potenciometro en AN0: el ADC da 0..1023 y el PWM toma 0..1023."""
from pypic.device import PIC16F887
from pypic.periph.adc import ADC, AN0
from pypic.periph.pwm import PWM1

CLOCK_HZ: Const[uint32] = 4_000_000


def main() -> None:
    ADC.start()
    # prescaler 1 y PR2 = 255: el periodo son 256 ciclos (3906,25 Hz a 4 MHz) y el ciclo de trabajo tiene 4 x 256 = 1024 cuentas,
    # justo la resolucion del ADC: una lectura es un ciclo de trabajo, sin escalar ni multiplicar
    PWM1.start(prescaler=1, period=255)
    while True:
        PWM1.duty(ADC.read(AN0))
