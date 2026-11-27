DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
BAUD 9600

RD0 = 0
b1 = 0

TMR2.every_ms 500

SERTXD("Hola PyxPic", CR, LF)

main:
  IF UART.rx_ready THEN
    GOSUB serial
  ENDIF

  IF TMR2.expired THEN
    TMR2.clear

    IF b1 = 1 THEN
      TOGGLE RD0
      SERTXD("LED: ", #RD0, CR, LF)
    ENDIF
  ENDIF

  GOTO main

serial:
  SERRXD(b0)

  IF b0 = "a" THEN
    b1 = 0
    RD0 = 1
    SERTXD("LED encendido", CR, LF)

  ELSEIF b0 = "b" THEN
    b1 = 0
    RD0 = 0
    SERTXD("LED apagado", CR, LF)

  ELSEIF b0 = "t" THEN
    b1 = 1
    RD0 = 1
    TMR2.clear
    SERTXD("Blink activado", CR, LF)

  ELSEIF b0 = "c" THEN
    b1 = 0
    RD0 = 0
    SERTXD("Blink cancelado", CR, LF)

  ENDIF

  RETURN