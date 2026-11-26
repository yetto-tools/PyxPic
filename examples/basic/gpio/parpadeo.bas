' Parpadeo en PyPIC BASIC (estilo PICAXE): RD0 cambia cada 500 ms.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

main:
  HIGH led
  PAUSE 500
  LOW led
  PAUSE 500
  GOTO main
