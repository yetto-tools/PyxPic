' Parpadeo de RD0 cada 500 ms con Timer2 en lugar de una espera bloqueante: 25 avisos de 20 ms.
' Equivale a examples/python/temporizadores/timer_blink.py.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

b0 = 0
TMR2.every_ms 20
main:
  IF TMR2.expired THEN
    TMR2.clear
    b0 = b0 + 1
    IF b0 = 25 THEN
      b0 = 0
      TOGGLE led
    ENDIF
  ENDIF
  GOTO main
