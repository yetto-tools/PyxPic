; Parpadeo de un LED en RD0 - PIC16F887, oscilador interno de 4 MHz (1 MHz de ciclo de instruccion).
; Es el mismo programa que examples/hex/blink_rd0.hex (verificado en Proteus), escrito en ensamblador.

STATUS  EQU 0x03        ; registro de estado (en todos los bancos)
PORTD   EQU 0x08        ; banco 0; en el banco 1 el mismo desplazamiento es TRISD (88h)
RP0     EQU 5           ; bit de seleccion de banco
C1      EQU 0x20        ; contadores de la espera, RAM de proposito general del banco 0
C2      EQU 0x21
C3      EQU 0x22

        ORG 0x0000
        GOTO main       ; vector de reset
        ORG 0x0004
        RETFIE          ; vector de interrupcion (no se habilitan interrupciones)
        ORG 0x0005
main:   BSF  STATUS,RP0
        BCF  PORTD,0    ; TRISD<0> = 0: RD0 como salida
        BCF  STATUS,RP0
        BCF  PORTD,0    ; LED apagado al inicio
loop:   BSF  PORTD,0
        CALL delay
        BCF  PORTD,0
        CALL delay
        GOTO loop

        ORG 0x0010
delay:  MOVLW 3         ; 3 x 256 x 256 iteraciones (~0,59 s)
        MOVWF C3
d3:     CLRF  C2
d2:     CLRF  C1
d1:     DECFSZ C1,F
        GOTO  d1
        DECFSZ C2,F
        GOTO  d2
        DECFSZ C3,F
        GOTO  d3
        RETURN

        ORG 0x2007      ; palabras de configuracion: CONFIG1 y CONFIG2
        DW 0x20D4, 0x3FFF
