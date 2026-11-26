' Lee un potenciometro en AN0 (RA0) y enciende RD0 cuando pasa de la mitad de la escala.
' Equivale a examples/python/adc/adc.py: READADC10 da 0 a 1023 y el reloj de conversion lo elige el compilador.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

main:
  READADC10 0, w3
  IF w3 > 511 THEN HIGH led ELSE LOW led
  GOTO main
