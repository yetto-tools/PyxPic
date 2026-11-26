' Eco: devuelve cada byte recibido sin modificar, a 9600 baudios 8N1 (RC6/TX, RC7/RX).
' Equivale a examples/python/uart/eco.py. Un desbordamiento (OERR) se borra para no quedarse mudo; los bytes perdidos se pierden.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
FROM pypic.periph.uart IMPORT UART

UART.start(baud = 9600)
main:
  IF UART.overrun() THEN UART.clear_overrun()
  IF UART.rx_ready() THEN UART.write(UART.recv())
  GOTO main
