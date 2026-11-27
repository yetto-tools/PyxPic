; Lee un potenciometro en AN0 (RA0) y enciende RD0 cuando pasa de la mitad de la escala (511) - PIC16F887, 4 MHz.
; Equivale a examples/python/adc/adc.py y a examples/basic/adc/adc.bas. Se ensambla con:
;
;   pyxpic asm adc.s -Wl,-presetVec=0h,-pintentry=4h -o adc.hex
;
; ADCON1: resultado a la derecha (ADFM = 1), referencias VDD y VSS. ADCON0: reloj FOSC/8 (TAD = 2 us a 4 MHz, dentro de 1,6 a 6 us),
; canal AN0 y ADC encendido. Antes de cada conversion se espera la adquisicion (~12 us; el valor tipico del datasheet es 11,5 us).

PROCESSOR 16F887
#include <xc.inc>

CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

PSECT udata_bank0
CNT: DS 1

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main

PSECT intentry,class=CODE,delta=2
    retfie

PSECT code
main:
    BANKSEL TRISD
    bcf BANKMASK(TRISD),TRISD_TRISD0_POSN             ; RD0 como salida
    BANKSEL ADCON1
    movlw 0x80                              ; ADFM = 1, VCFG = 00 (VDD y VSS)
    movwf BANKMASK(ADCON1)
    BANKSEL ADCON0
    movlw 0x41                              ; ADCS = 01 (FOSC/8), CHS = 0000 (AN0), ADON = 1
    movwf ADCON0
    BANKSEL PORTD
    bcf PORTD,PORTD_RD0_POSN
loop:
    movlw 4                                 ; adquisicion: ~12 us
    movwf CNT
adquirir:
    decfsz CNT,f
    goto adquirir
    bsf ADCON0,ADCON0_GO_DONE_POSN         ; GO
espera:
    btfsc ADCON0,ADCON0_GO_DONE_POSN       ; hasta que la conversion termine
    goto espera
    movf ADRESH,w                           ; con el resultado a la derecha ADRESH tiene los bits 9:8: valor > 511 si ADRESH >= 2
    sublw 1                                 ; W = 1 - ADRESH; C = 1 si ADRESH <= 1
    btfsc STATUS,STATUS_C_POSN
    goto apagado
    bsf PORTD,PORTD_RD0_POSN
    goto loop
apagado:
    bcf PORTD,PORTD_RD0_POSN
    goto loop
END
