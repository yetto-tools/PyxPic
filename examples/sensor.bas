' Lee el canal AN0 y enciende RD0 si pasa de la mitad (512 de 1023); RD1 refleja el boton de RB0.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL alarma = D.0
SYMBOL aviso = D.1
SYMBOL boton = pinB.0

main:
  READADC10 0, w3
  IF w3 > 511 THEN
    HIGH alarma
  ELSE
    LOW alarma
  ENDIF
  IF boton = 1 THEN HIGH aviso ELSE LOW aviso
  PAUSE 50
  GOTO main
