; =====================================================
; Programa: BLINK LED con PIC 12F675
; Descripción: Parpadea un LED conectado a GP0
; Oscilador: 4 MHz (interno)
; =====================================================

    list p=12f675
    include "p12f675.inc"

; Configuración de Fuses
    __CONFIG _CP_OFF & _CPD_OFF & _BOREN_ON & _MCLRE_ON & _WDT_OFF & _PWRTE_ON & _INTRC_OSC_NOCLKOUT

; =====================================================
; VARIABLES
; =====================================================
    CBLOCK 0x20
        contador1       ; Contador 1 para delay
        contador2       ; Contador 2 para delay
        contador3       ; Contador 3 para delay
    ENDC

; =====================================================
; VECTOR DE RESET
; =====================================================
    org 0x000
    goto INICIO

; =====================================================
; PROGRAMA PRINCIPAL
; =====================================================
    org 0x004

INICIO:
    ; Configurar GPIO
    movlw 0xB7        ; 10110111 - Solo GP0 como salida (bit 0 = salida)
    movwf TRISIO      ; Configurar dirección de pines
    
    ; Deshabilitar comparadores analógicos
    movlw 0x07        ; Todos los pines como digitales
    movwf CMCON       ; Registro de control de comparadores
    
    ; Limpiar puerto GPIO
    clrf GPIO
    
; =====================================================
; LOOP PRINCIPAL
; =====================================================
LOOP:
    ; LED ON
    bsf GPIO, 0       ; Poner GP0 en alto
    call DELAY_500MS  ; Esperar 500 ms
    
    ; LED OFF
    bcf GPIO, 0       ; Poner GP0 en bajo
    call DELAY_500MS  ; Esperar 500 ms
    
    goto LOOP         ; Repetir indefinidamente

; =====================================================
; SUBRUTINA: DELAY 500ms
; =====================================================
; Genera un retardo de aproximadamente 500 ms
; Oscilador interno 4 MHz (Fosc/4 = 1 MHz = 1 µs por instrucción)
; 500 ms = 500,000 µs

DELAY_500MS:
    movlw 0x03        ; Cargar contador1
    movwf contador1
    
LOOP_1:
    movlw 0xFF        ; Cargar contador2
    movwf contador2
    
LOOP_2:
    movlw 0xFA        ; Cargar contador3 (250 iteraciones)
    movwf contador3
    
LOOP_3:
    nop                 ; 1 ciclo
    nop                 ; 1 ciclo
    nop                 ; 1 ciclo
    decfsz contador3, F ; 1 ciclo
    goto LOOP_3         ; 2 ciclos
    
    decfsz contador2, F ; 1 ciclo
    goto LOOP_2         ; 2 ciclos
    
    decfsz contador1, F ; 1 ciclo
    goto LOOP_1         ; 2 ciclos
    
    return

    end
