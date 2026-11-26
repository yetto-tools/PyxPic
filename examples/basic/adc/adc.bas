' Lee un potenciometro en AN0 (RA0) y enciende RD0 cuando pasa de la mitad de la escala.
' Equivale a examples/python/adc/adc.py: ADC.read(AN0) da 0 a 1023 y el reloj de conversion lo elige el compilador.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
FROM pypic.periph.adc IMPORT ADC, AN0

SYMBOL led = D.0

ADC.start()             ' resultado de 10 bits a la derecha, referencias VDD y VSS
main:
  w3 = ADC.read(AN0)    ' canal, adquisicion, conversion y resultado
  IF w3 > 511 THEN HIGH led ELSE LOW led
  GOTO main
