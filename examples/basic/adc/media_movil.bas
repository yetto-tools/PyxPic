' Media movil de las ultimas 8 lecturas del ADC (AN0) con un arreglo circular; enciende RD0 si la media pasa de la mitad.
' Equivale a examples/python/adc/media_movil.py: el arreglo guarda los 8 bits altos de cada lectura y el indice se acota con AND 7.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
FROM pypic.periph.adc IMPORT ADC, AN0

SYMBOL led = D.0
DIM muestras(8) AS BYTE
DIM pos AS BYTE
DIM suma AS WORD
DIM media AS BYTE

ADC.start()
main:
  w3 = ADC.read(AN0)
  muestras(pos) = w3 >> 2
  pos = (pos + 1) & 7
  suma = 0
  FOR b0 = 0 TO 7
    suma = suma + muestras(b0)
  NEXT b0
  media = suma >> 3
  IF media > 127 THEN HIGH led ELSE LOW led
  GOTO main
