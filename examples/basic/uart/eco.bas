' Eco: devuelve cada byte recibido sin modificar, a 9600 baudios 8N1 (RC6/TX, RC7/RX).
' Equivale a examples/python/uart/eco.py, salvo que no trata el desbordamiento del receptor (OERR): con UART.overrun y UART.clear_overrun si.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

main:
  SERRXD(b0)            ' espera un byte
  SERTXD(b0)            ' y lo envia tal cual
  GOTO main
