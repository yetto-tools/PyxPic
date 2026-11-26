; Parpadeo de un LED en RD0 - PIC16F887, oscilador interno de 4 MHz (1 MHz de ciclo de instruccion).
; Es el mismo programa que examples/hex/blink_rd0.hex (verificado en Proteus), escrito con la sintaxis
; de MPLAB XC8 PIC Assembler (pic-as). Se ensambla con:
;
;   pypic asm blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o blink_rd0.hex
;
; (-mcpu=16F887 se toma de la directiva PROCESSOR; -p fija donde va cada psect, igual que en pic-as.)

PROCESSOR 16F887
#include <xc.inc>

; palabras de configuracion (nombres de los ajustes del DFP)
CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

PSECT udata_bank0       ; contadores de la espera, RAM de proposito general del banco 0
C1: DS 1
C2: DS 1
C3: DS 1

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main           ; vector de reset

PSECT intentry,class=CODE,delta=2
    retfie              ; vector de interrupcion (no se habilitan interrupciones)

PSECT code
main:
    bsf STATUS,STATUS_RP0_POSN
    bcf BANKMASK(TRISD),TRISD_TRISD0_POSN   ; TRISD<0> = 0: RD0 como salida
    bcf STATUS,STATUS_RP0_POSN
    bcf PORTD,PORTD_RD0_POSN                ; LED apagado al inicio
loop:
    bsf PORTD,PORTD_RD0_POSN
    call delay
    bcf PORTD,PORTD_RD0_POSN
    call delay
    goto loop

    ORG 0x0B            ; desplazamiento dentro del psect: la espera queda en 0x10
delay:
    movlw 3             ; 3 x 256 x 256 iteraciones (~0,59 s)
    movwf C3
d3: clrf C2
d2: clrf C1
d1: decfsz C1,f
    goto d1
    decfsz C2,f
    goto d2
    decfsz C3,f
    goto d3
    return
