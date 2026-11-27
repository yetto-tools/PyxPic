; Un pulsador en RB0 enciende el LED de RD0 (sin esperas: solo lee y escribe pines) - PIC16F887, oscilador interno de 4 MHz.
; Equivale a examples/python/gpio/button_led.py y a examples/basic/gpio/boton_led.bas. Se ensambla con:
;
;   pyxpic asm boton_led.s -Wl,-presetVec=0h,-pintentry=4h -o boton_led.hex

PROCESSOR 16F887
#include <xc.inc>

; palabras de configuracion (nombres de los ajustes del DFP)
CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main           ; vector de reset

PSECT intentry,class=CODE,delta=2
    retfie              ; vector de interrupcion (no se habilitan interrupciones)

PSECT code
main:
    BANKSEL TRISD
    bcf BANKMASK(TRISD),TRISD_TRISD0_POSN             ; RD0 como salida
    bsf BANKMASK(TRISB),TRISB_TRISB0_POSN             ; RB0 como entrada
    BANKSEL ANSELH
    bcf BANKMASK(ANSELH),ANSELH_ANS12_POSN            ; RB0 es AN12: tras el reset es analogico y se lee como 0; se pasa a digital
    BANKSEL PORTD
    bcf PORTD,PORTD_RD0_POSN                ; LED apagado al inicio
loop:
    btfss PORTB,PORTB_RB0_POSN              ; boton suelto: apagar
    goto apagado
    bsf PORTD,PORTD_RD0_POSN
    goto loop
apagado:
    bcf PORTD,PORTD_RD0_POSN
    goto loop
END
