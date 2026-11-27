; Cuenta los arranques en la EEPROM (direccion 0) y parpadea RD0 tantas veces como arranques lleva - PIC16F887, 4 MHz.
; Equivale a examples/python/eeprom/eeprom_arranques.py y a examples/basic/eeprom/eeprom_arranques.bas. Se ensambla con:
;
;   pyxpic asm eeprom_arranques.s -Wl,-presetVec=0h,-pintentry=4h -o eeprom_arranques.hex
;
; Lectura (DS40001291H 10.1.2): EEADR, EEPGD = 0 y RD = 1. Escritura (10.1.3): EEADR, EEDAT, WREN = 1, 55h y AAh a EECON2 y WR = 1; este
; programa nunca habilita las interrupciones, asi que no hace falta bajar GIE antes de la secuencia.

PROCESSOR 16F887
#include <xc.inc>

CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

; EECON2 no es un registro con bits (solo recibe la secuencia 55h/AAh): el DFP no define su simbolo
EECON2 EQU 0x18D

PSECT udata_bank0
N: DS 1
VECES: DS 1
C1: DS 1
C2: DS 1

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main

PSECT intentry,class=CODE,delta=2
    retfie

PSECT code
main:
    BANKSEL TRISD
    bcf BANKMASK(TRISD),TRISD_TRISD0_POSN             ; RD0 como salida
    BANKSEL EEADR
    clrf BANKMASK(EEADR)                              ; direccion 0
    BANKSEL EECON1
    bcf BANKMASK(EECON1),EECON1_EEPGD_POSN            ; memoria de datos
    bsf BANKMASK(EECON1),EECON1_RD_POSN               ; leer
    BANKSEL EEDAT
    movf BANKMASK(EEDAT),w                            ; una EEPROM virgen vale 0xFF
    BANKSEL N
    movwf N
    incf N,f                                ; 0xFF + 1 = 0: se arranca en 1
    btfsc STATUS,STATUS_Z_POSN
    incf N,f
    ; escribir N en la direccion 0
    BANKSEL EEADR
    clrf BANKMASK(EEADR)
    BANKSEL N
    movf N,w
    BANKSEL EEDAT
    movwf BANKMASK(EEDAT)
    BANKSEL EECON1
    bcf BANKMASK(EECON1),EECON1_EEPGD_POSN
    bsf BANKMASK(EECON1),EECON1_WREN_POSN
    movlw 0x55
    movwf BANKMASK(EECON2)
    movlw 0xAA
    movwf BANKMASK(EECON2)
    bsf BANKMASK(EECON1),EECON1_WR_POSN
esperar_escritura:
    btfsc BANKMASK(EECON1),EECON1_WR_POSN             ; hasta 6 ms
    goto esperar_escritura
    bcf BANKMASK(EECON1),EECON1_WREN_POSN
    ; parpadear N veces
    BANKSEL VECES
    clrf VECES
parpadeo:
    movf N,w
    subwf VECES,w                           ; W = VECES - N; C = 1 si VECES >= N
    btfsc STATUS,STATUS_C_POSN
    goto fin
    BANKSEL PORTD
    bsf PORTD,PORTD_RD0_POSN
    call espera
    bcf PORTD,PORTD_RD0_POSN
    call espera
    BANKSEL VECES
    incf VECES,f
    goto parpadeo
fin:
    goto fin

espera:                                     ; ~197 ms: 256 x (256 x 3 ciclos)
    clrf C2
e2: clrf C1
e1: decfsz C1,f
    goto e1
    decfsz C2,f
    goto e2
    return
END
