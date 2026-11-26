' Parpadeo de RD0 con un cristal de 20 MHz en OSC1/OSC2: 2000 ms encendido y 2000 ms apagado, exactos al ciclo de instruccion.
' Equivale a examples/python/reloj/reloj_cristal.py. Con cristal, RA6 y RA7 son del oscilador. NO probado en hardware, solo en simulador.
DEVICE PIC16F887
CLOCK CRYSTAL 20 MHZ

SYMBOL led = D.0

main:
  HIGH led
  PAUSE 2000
  LOW led
  PAUSE 2000
  GOTO main
