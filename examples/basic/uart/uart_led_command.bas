' Comandos por la UART a 9600 baudios 8N1 (RC6/TX, RC7/RX): "a" enciende RB0 y "b" o "c" lo apagan.
' Cada tecla recibida responde con el valor de RB0 y vuelve a imprimir el saludo.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
BAUD 9600             ' velocidad de la UART (8N1); sin esta linea son 9600

RB0 = 0
main:
  SERTXD("Hola PyxPic ", #RB0, CR, LF)
  GOSUB serial
  GOTO main

serial:
  SERRXD(b0)                      ' espera un byte por la UART (RC7/RX) y lo guarda en b0
  IF b0 = "a" THEN
    SERTXD("Boton A presionado. Valor: ", #RB0, CR, LF)
    RB0 = 1
  ELSEIF b0 = "b" THEN
    SERTXD("Boton B presionado. Valor: ", #RB0, CR, LF)
    RB0 = 0
  ELSEIF b0 = "c" THEN
    SERTXD("Boton C presionado. Valor: ", #RB0, CR, LF)
    RB0 = 0
  ENDIF
  RETURN
