' Media movil de las ultimas 8 lecturas del ADC (AN0) con un arreglo circular; enciende RD0 si la media pasa de la mitad.
' Equivale a examples/python/adc/media_movil.py: el arreglo guarda los 8 bits altos de cada lectura y el indice se acota con AND 7.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0
DIM muestras(8) AS BYTE
DIM pos AS BYTE
DIM suma AS WORD
DIM media AS BYTE

main:
  READADC 0, b0                 ' los 8 bits de arriba de los 10
  muestras(pos) = b0
  pos = (pos + 1) & 7
  suma = 0
  FOR b1 = 0 TO 7
    suma = suma + muestras(b1)
  NEXT b1
  media = suma >> 3
  IF media > 127 THEN HIGH led ELSE LOW led
  GOTO main
