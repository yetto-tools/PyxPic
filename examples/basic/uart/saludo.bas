' Saludo por el puerto serie cada segundo, y el valor de un contador en decimal: "Hola PyPIC 1", "Hola PyPIC 2"...
' Variante con contador de examples/python/uart/saludo_texto.py. SERTXD acepta textos, #valor (en decimal), CR, LF y bytes sueltos.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

b0 = 0
main:
  b0 = b0 + 1
  SERTXD("Hola PyPIC ", #b0, CR, LF)
  PAUSE 1000
  GOTO main
