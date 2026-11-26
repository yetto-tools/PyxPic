' Comparador 1 contra CVREF: enciende RD0 cuando la tension en RA0 supera la mitad de VDD (un comparador a secas, sin histeresis).
' Equivale a examples/python/comparadores/comparador.py. Los comparadores no existen en PICAXE: se manejan con OBJETO.metodo argumentos.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

CVREF.start step = 8                                  ' VDD/4 + 8/32 x VDD = VDD/2
CMP1.start negative = C12IN0, positive = REF_CVREF    ' C1OUT = 1 cuando CVREF > RA0
main:
  IF CMP1.out THEN LOW led ELSE HIGH led
  GOTO main
