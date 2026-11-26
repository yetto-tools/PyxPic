' Regula el brillo de un LED en RC2 (PWM1) con un potenciometro en AN0: el ADC da 0 a 1023 y el PWM toma 0 a 1023.
' Equivale a examples/python/pwm/pwm.py: prescaler 1 y PR2 = 255 dan 1024 cuentas de ciclo de trabajo, una lectura es un ciclo.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
FROM pypic.periph.adc IMPORT ADC, AN0
FROM pypic.periph.pwm IMPORT PWM1

ADC.start()
PWM1.start(prescaler = 1, period = 255)
main:
  PWM1.duty(ADC.read(AN0))
  GOTO main
