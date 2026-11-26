' Regula el brillo de un LED en RC2 (PWM1) con un potenciometro en AN0: el ADC da 0 a 1023 y el PWM toma 0 a 1023.
' Equivale a examples/python/pwm/pwm.py. PWMOUT pin, periodo, ciclo: el periodo es PR2 (255 = 256 ciclos de instruccion, prescaler 1) y el
' ciclo de trabajo tiene 4 x (PR2 + 1) = 1024 cuentas, justo la resolucion del ADC.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

main:
  READADC10 0, w0
  PWMOUT C.2, 255, w0
  GOTO main
