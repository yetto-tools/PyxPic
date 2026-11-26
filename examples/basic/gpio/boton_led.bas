' Un pulsador en RB0 enciende el LED de RD0 (sin esperas: solo lee y escribe pines).
' Equivale a examples/python/gpio/button_led.py.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0
SYMBOL boton = pinB.0

main:
  IF boton = 1 THEN HIGH led ELSE LOW led
  GOTO main
