; Regula el brillo de un LED en RC2 (PWM1) con un potenciometro en AN0: el ADC da 0 a 1023 y el PWM toma 0 a 1023 - PIC16F887, 4 MHz.
; Equivale a examples/python/pwm/pwm.py y a examples/basic/pwm/pwm.bas. Se ensambla con:
;
;   pypic asm pwm.s -Wl,-presetVec=0h,-pintentry=4h -o pwm.hex
;
; PR2 = 255 con prescaler 1: periodo de 256 ciclos y 1024 cuentas de ciclo de trabajo. El resultado del ADC va justificado a la izquierda
; (ADFM = 0): ADRESH son los 8 bits altos (CCPR1L) y ADRESL<7:6> los 2 bajos (DC1B, CCP1CON<5:4>). Orden de arranque de DS40001291H 11.5.7:
; pin como entrada, PR2, CCP1CON, ciclo de trabajo, Timer2 y, tras el primer periodo, el pin como salida.

PROCESSOR 16F887
#include <xc.inc>

CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF
CONFIG BOREN = OFF, IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF
CONFIG CONFIG2 = 0x3FFF

PSECT udata_bank0
CNT: DS 1
BAJOS: DS 1

PSECT resetVec,class=CODE,delta=2
resetVec:
    goto main

PSECT intentry,class=CODE,delta=2
    retfie

PSECT code
main:
    BANKSEL TRISC
    bsf BANKMASK(TRISC),TRISC_TRISC2_POSN             ; 1. RC2 como entrada mientras se configura
    BANKSEL PR2
    movlw 255
    movwf BANKMASK(PR2)                               ; 2. periodo
    BANKSEL ADCON1
    clrf BANKMASK(ADCON1)                             ; ADFM = 0 (a la izquierda), VDD y VSS
    BANKSEL CCP1CON
    movlw 0x0C                              ; 3. modo PWM (CCP1M = 1100), DC1B = 00
    movwf CCP1CON
    clrf CCPR1L                             ; 4. ciclo de trabajo inicial 0
    movlw 0x41                              ; ADC: FOSC/8, AN0, encendido
    movwf ADCON0
    bcf PIR1,PIR1_TMR2IF_POSN               ; 5. Timer2 encendido con prescaler 1
    movlw 0x04
    movwf T2CON
primer_periodo:
    btfss PIR1,PIR1_TMR2IF_POSN             ; 6. esperar el primer periodo completo
    goto primer_periodo
    BANKSEL TRISC
    bcf BANKMASK(TRISC),TRISC_TRISC2_POSN             ; y solo entonces habilitar la salida
    BANKSEL ADCON0
loop:
    movlw 4                                 ; adquisicion ~12 us
    movwf CNT
adquirir:
    decfsz CNT,f
    goto adquirir
    bsf ADCON0,ADCON0_GO_DONE_POSN
espera:
    btfsc ADCON0,ADCON0_GO_DONE_POSN
    goto espera
    movf ADRESH,w
    movwf CCPR1L                            ; los 8 bits altos del ciclo de trabajo
    BANKSEL ADRESL
    movf BANKMASK(ADRESL),w                           ; los 2 bajos estan en ADRESL<7:6>
    BANKSEL BAJOS
    movwf BAJOS
    bcf STATUS,STATUS_C_POSN
    rrf BAJOS,f                             ; bits 7:6 -> 5:4 en dos desplazamientos
    bcf STATUS,STATUS_C_POSN
    rrf BAJOS,f
    movlw 0x30
    andwf BAJOS,w
    iorlw 0x0C                              ; modo PWM
    movwf CCP1CON
    goto loop
END
