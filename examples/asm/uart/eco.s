; Eco: devuelve cada byte recibido sin modificar, a 9600 baudios 8N1 (RC6/TX, RC7/RX) - PIC16F887, 4 MHz.
; Equivale a examples/python/uart/eco.py y a examples/basic/uart/eco.bas. Se ensambla con:
;
;   pyxpic asm eco.s -Wl,-presetVec=0h,-pintentry=4h -o eco.hex
;
; BRGH = 1 y SPBRG = 25: 4 MHz / (16 x 26) = 9615 baudios (0,16 % de error). TRISC<7:6> quedan como entradas: el EUSART toma los pines.
; Un desbordamiento del receptor (OERR) lo detiene hasta bajar CREN y subirlo otra vez; los bytes que llegaron mientras tanto se pierden.

PROCESSOR 16F887
#include <xc.inc>

CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main

PSECT intentry,class=CODE,delta=2
    retfie

PSECT code
main:
    BANKSEL SPBRG
    movlw 25
    movwf BANKMASK(SPBRG)
    movlw 0x24                              ; TXEN = 1, BRGH = 1 (modo asincrono)
    movwf BANKMASK(TXSTA)
    BANKSEL RCSTA
    movlw 0x90                              ; SPEN = 1, CREN = 1 (8 bits)
    movwf RCSTA
loop:
    btfss RCSTA,RCSTA_OERR_POSN
    goto sin_desbordamiento
    bcf RCSTA,RCSTA_CREN_POSN               ; borrar el desbordamiento
    bsf RCSTA,RCSTA_CREN_POSN
sin_desbordamiento:
    btfss PIR1,PIR1_RCIF_POSN               ; hay un byte recibido?
    goto loop
    movf RCREG,w                            ; leerlo (baja RCIF)
transmitir:
    btfss PIR1,PIR1_TXIF_POSN               ; esperar el registro de transmision libre
    goto transmitir
    movwf TXREG
    goto loop
END
