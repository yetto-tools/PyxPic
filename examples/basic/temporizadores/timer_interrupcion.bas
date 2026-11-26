' Parpadeo de RD0 cada 500 ms con la interrupcion de Timer2 (25 avisos de 20 ms): main no hace nada, todo ocurre en la interrupcion.
' ON INTERRUPT dice que la etiqueta aviso es la rutina de interrupcion; termina en RETURN. Comparte la variable b0 con el resto del programa.
' Equivale a examples/python/temporizadores/timer_blink.py, con la interrupcion en lugar de la consulta de la bandera.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

ON INTERRUPT aviso
b0 = 0
TMR2.every_ms 20          ' un aviso cada 20 ms
TMR2.interrupt_on         ' habilita la interrupcion de Timer2 (y PEIE)
ENABLE INTERRUPTS         ' GIE
main:
  GOTO main

aviso:
  TMR2.clear              ' baja la bandera
  b0 = b0 + 1
  IF b0 = 25 THEN
    b0 = 0
    TOGGLE led
  ENDIF
  RETURN
