BASM — BASIC ASSEMBLY
Documento de contexto y propuesta técnica para desarrollo

1. OBJETIVO
Crear BASM, un lenguaje híbrido inspirado en BASIC, PICAXE y Assembly, inicialmente dirigido al PIC16F887. Debe simplificar la programación sin impedir el acceso directo al hardware. Se compilará a código máquina nativo: no se pretende implementar un intérprete BASIC, una máquina virtual ni un intérprete Python en el microcontrolador.
BASM podrá funcionar independientemente de PyPIC, compartiendo backend, ensamblador, SDK, simulador y herramientas de depuración. La dependencia de XC8, MPASM o PIC-AS será opcional mediante backends externos. Nombre propuesto del compilador: basmc; extensión: .basm.

2. MOTIVACIÓN: PICAXE Y ASSEMBLY
PICAXE ofrece instrucciones BASIC de alto nivel, variables compartidas y descarga serial mediante firmware residente. BASM adopta la ergonomía, pero no el intérprete: la traducción ocurre en el equipo de desarrollo. Un bootloader nativo será opcional.

Ejemplo original de blink en MPASM, PIC16F887, reloj interno de 4 MHz:

        LIST P=16F887
        INCLUDE "P16F887.INC"
        __CONFIG _CONFIG1, _INTRC_OSC_NOCLKOUT & _WDT_OFF & _LVP_OFF
        __CONFIG _CONFIG2, _BOR40V & _WRT_OFF
        CBLOCK 0x20
            CNT1
            CNT2
            CNT3
        ENDC
        ORG 0x0000
        GOTO INIT
INIT:
        BANKSEL OSCCON
        MOVLW b'01100001'
        MOVWF OSCCON
        BANKSEL ANSEL
        CLRF ANSEL
        CLRF ANSELH
        BANKSEL PORTB
        CLRF PORTB
        BANKSEL TRISB
        BCF TRISB, 0
        BANKSEL PORTB
LOOP:
        BSF PORTB, 0
        CALL DELAY_500MS
        BCF PORTB, 0
        CALL DELAY_500MS
        GOTO LOOP
DELAY_500MS:
        MOVLW D'3'
        MOVWF CNT3
D3:
        MOVLW D'250'
        MOVWF CNT2
D2:
        MOVLW D'221'
        MOVWF CNT1
D1:
        DECFSZ CNT1, F
        GOTO D1
        DECFSZ CNT2, F
        GOTO D2
        DECFSZ CNT3, F
        GOTO D3
        RETURN
        END

Retardo aproximado; verificar en simulador y hardware. RB0 es el pin 33 en DIP-40. LED conectado mediante resistencia de 330 ohmios a GND. Configurar RB0 como digital y salida.

Equivalente propuesto en BASM:

DEVICE PIC16F887
CLOCK 4MHz INTERNAL
CONFIG RB0, OUTPUT
MAIN:
    HIGH RB0
    PAUSE 500
    LOW RB0
    PAUSE 500
    GOTO MAIN
END

3. MODELO DE PROGRAMACIÓN
Nivel 1: instrucciones simples estilo PICAXE (HIGH, LOW, PAUSE).
Nivel 2: control avanzado de periféricos, variables, interrupciones y memoria.
Nivel 3: Assembly nativo, acceso directo a registros y control de ciclos.
Los tres niveles podrán coexistir. Ejemplo:

ASM
    BANKSEL PORTB
    BSF PORTB, 0
    NOP
    BCF PORTB, 0
ENDASM

Para optimización segura, contemplar ASM_NATIVE CLOBBERS(W, STATUS, TRISB) ... ENDASM. Un bloque nativo sin declaraciones deberá tratarse conservadoramente. El compilador deberá restablecer bancos y páginas cuando corresponda, sin alterar la semántica explícita del programador.

4. VARIABLES COMPARTIDAS INSPIRADAS EN PICAXE
BIT: 1 bit, 0..1.
BYTE / bN: 8 bits, 0..255.
WORD / wN: 16 bits, 0..65535.
DWORD / dN: 32 bits, 0..4294967295.
QWORD / qN: 64 bits, 0..18446744073709551615.

Modelo little-endian propuesto: w0 comparte b0 (bajo) y b1 (alto); w1 comparte b2 y b3; d0 comparte w0 y w1; q0 comparte d0 y d1. bit0..bit7 corresponden a b0, bit8..bit15 a b1, etc. El índice máximo dependerá del dispositivo y la asignación de memoria. Los nombres son alias de almacenamiento, no registros físicos adicionales. W es el acumulador nativo y no debe confundirse con w0.

Mapa:
q0: [ b0 b1 b2 b3 b4 b5 b6 b7 ]
d0: [ b0 b1 b2 b3 ]   d1: [ b4 b5 b6 b7 ]
w0: [ b0 b1 ]  w1: [ b2 b3 ]  w2: [ b4 b5 ]  w3: [ b6 b7 ]

MOV w0, 1023   ; b0=255, b1=3
MOV b0, 0      ; w0=768
SET bit0       ; w0=769
INC w0         ; w0=770

El compilador debe conocer todos los alias y evitar optimizaciones que ignoren solapamientos. Reservar almacenamiento según las vistas utilizadas, respetando su relación; no reservar toda la memoria virtual innecesariamente. Las variables multibyte pueden cruzar bancos y requieren generación de código adecuada.

5. VARIABLES TIPADAS Y ALIAS
Además del modelo compartido, permitir variables independientes con asignación automática de RAM:
SYMBOL contador = b0
SYMBOL lectura = w1
SYMBOL led = RB0
ALIAS temperatura = w0
ALIAS encendido = bit24
VAR velocidad : U16
VAR distancia : U32
VAR estado : BOOL
VAR error : I8
VAR buffer : BYTE[32]
VAR lecturas : WORD[10]
VAR nombre : CHAR[16]

Tipos propuestos: U8/U16/U32/U64, I8/I16/I32/I64, BOOL, BIT, BYTE, WORD, DWORD, QWORD, CHAR. Constantes, arreglos, punteros y estructuras (estas últimas en fases posteriores). Definir reglas explícitas de conversiones, signo, promociones, desbordamiento, división por cero y desplazamientos. No depender de asignación dinámica de memoria. Operaciones de 32 y 64 bits serán software y opcionales según necesidades.

6. INSTRUCCIONES Y SINTAXIS PROPUESTAS
Directivas: DEVICE, CLOCK, CONFIG, CONST, SYMBOL, ALIAS, VAR, END.
Aritmética: MOV, ADD, SUB, MUL, DIV, MOD, INC, DEC, CMP.
Lógica: AND, OR, XOR, NOT, SHL, SHR, SET, CLR, TOGGLE.
Control: IF/THEN/ELSE/ENDIF, FOR, WHILE, SELECT, GOTO, CALL, GOSUB, RETURN.
GPIO: HIGH, LOW, TOGGLE, INPUT, OUTPUT, GPIO.
Periféricos: ADC_INIT, READADC, PWM, SERIN, SEROUT, I2C_INIT, I2CREAD, SPIWRITE, I2C_SCAN.
Tiempo: PAUSE, PAUSEUS, TIMER, TIMER2_INIT, ON_INTERRUPT.
Memoria: PEEK, POKE, READ, WRITE (definir espacios de memoria explícitos).
Modularidad: IMPORT, INCLUDE, MACRO; módulos Assembly externos.
Bajo nivel: ASM/ENDASM, ASM_NATIVE y todas las instrucciones soportadas por el dispositivo.

Ejemplo ADC:
DEVICE PIC16F887
CLOCK 4MHz INTERNAL
SYMBOL contador = b0
SYMBOL lectura = w1
SYMBOL led = RB0
CONFIG led, OUTPUT
ADC_INIT AN0
MAIN:
    READADC AN0, lectura
    IF lectura > 511 THEN
        HIGH led
        INC contador
    ELSE
        LOW led
    ENDIF
    PAUSE 100
    GOTO MAIN
END

7. ARQUITECTURA DEL COMPILADOR
Frontends: BASM, PyPIC (subconjunto de sintaxis Python), programación visual por bloques.
Pipeline: lexer/parser -> AST -> análisis semántico y tipos -> IR común -> optimizador -> asignación de memoria y recursos -> backend específico de arquitectura -> ensamblador/enlazador -> Intel HEX y archivos auxiliares.
Soportar salida Assembly legible, listado de instrucciones, mapa de símbolos, reporte RAM/Flash, información de depuración y, opcionalmente, BIN con formato documentado para palabras de programa de 14 bits.
Desarrollar ensamblador nativo independiente. Permitir adaptadores opcionales para herramientas externas y transpilación a C/Assembly cuando proceda.
Las pseudoinstrucciones podrán expandirse inline o llamar subrutinas reutilizables. Incluir únicamente librerías utilizadas. Las operaciones de ancho superior a 8 bits deberán propagar acarreo y manejar signo correctamente.

8. LIMITACIONES DEL PIC16F887
Flash: 8192 palabras de 14 bits. RAM: 368 bytes. EEPROM: 256 bytes. Pila de retorno hardware: 8 niveles. Frecuencia máxima: 20 MHz según condiciones eléctricas; reloj interno usado en ejemplos: 4 MHz. Arquitectura de 8 bits, 35 instrucciones, 4 bancos de registros y un vector de interrupción.
Gestionar BANKSEL, PCLATH y páginas de programa, profundidad de llamadas y uso de pila por interrupciones. No asumir que toda la RAM es contigua. Los accesos a PORT pueden presentar problemas read-modify-write; considerar registros sombra cuando corresponda.
Operaciones multibyte no atómicas: proteger accesos compartidos con interrupciones cuando sea necesario. Manejar contexto W, STATUS y PCLATH de ISR. Las interrupciones comparten vector y deben despacharse según flags.
Los retardos PAUSE bloquean la CPU; ofrecer temporizadores y mecanismos no bloqueantes. Ejemplo conceptual:
TIMER T1, 500ms, PERIODIC
ON_INTERRUPT T1:
    TOGGLE RB0
    RETI
RETI es pseudoinstrucción propuesta; la instrucción nativa PIC es RETFIE.

9. GESTIÓN DE RECURSOS Y DIAGNÓSTICOS
Detectar conflictos de pines, modos analógicos/digitales, temporizadores, PWM, UART, SPI e I2C. Ejemplo: TIMER2 no puede reconfigurarse de manera incompatible mientras PWM lo utiliza. Permitir compartir recursos cuando sus configuraciones sean compatibles.
Diagnósticos con archivo, línea, código de error, descripción y solución. Validar pines/periféricos disponibles, capacidad RAM/Flash, profundidad de pila, desbordamientos y rangos de constantes. Reportar ciclos de ejecución cuando sea calculable; distinguir estimaciones de garantías.

10. SOPORTE MULTIFAMILIA
No codificar toda la lógica exclusivamente para PIC16F887. Separar descripciones de dispositivos (SFR, bits, pines, bancos, memoria, vectores, periféricos, configuración y capacidades) de las reglas del lenguaje.
Objetivos futuros: PIC baseline (PIC10/12), mid-range clásico (PIC16F887), enhanced mid-range y PIC18. El soporte del PIC12F625 puede considerarse tras el primer objetivo de PyPIC. Validar cada modelo y sus capacidades antes de habilitar funciones. No prometer compatibilidad universal automática.

11. BOOTLOADER OPCIONAL
Instalación inicial por ICSP/PICkit; descarga posterior por UART mediante USB-TTL con niveles compatibles. Escribir código máquina en Flash, no tokens BASIC. Considerar identificación del dispositivo, CRC, validación de rangos, protección de la región del bootloader, vectores, recuperación ante fallos y verificación posterior. Reservar Flash y pines cuando sea necesario. No sustituye ICSP para recuperación ni para todas las configuraciones. El firmware debe poder funcionar sin bootloader.

12. ECOSISTEMA Y HERRAMIENTAS
PyPIC: frontend de sintaxis Python compilada, no intérprete.
BASM: frontend BASIC Assembly independiente.
basmc: CLI propuesta.
BASM SDK: librerías de GPIO, ADC, PWM, temporizadores, UART, SPI, I2C, EEPROM y utilidades.
PyPIC Blocks: programación visual.
PyPIC Boot: bootloader opcional.
Extensión VS Code: autocompletado, resaltado, diagnósticos, compilación, selección de dispositivo, depuración, breakpoints y vista pinout inspirada en PICAXE. Wizard de proyectos nuevos y migración de dispositivo/backend. Terminal serial y escáner I2C (mediante firmware o adaptador compatible). Diferenciar simulación y depuración en hardware: esta última depende de interfaces y recursos físicos.

13. HOJA DE RUTA PROPUESTA
0.1 Ensamblador nativo PIC16F887 y pruebas de codificación.
0.2 Pseudoinstrucciones, macros, variables compartidas y tipadas.
0.3 GPIO, ADC y temporizadores.
0.4 UART, SPI, I2C y PWM.
0.5 Optimización, gestión de recursos, memoria y diagnósticos.
0.6 Simulador, depuración y extensión IDE.
0.7 Bootloader opcional.
1.0 Integración completa con PyPIC y SDK estable.
1.5 Soporte multifamilia.
2.0 Programación visual por bloques.

14. CRITERIOS DE VALIDACIÓN
Comparar opcodes y archivos HEX contra herramientas de referencia. Pruebas de límites 8/16/32/64 bits, alias b/w/d/q/bit, signo, acarreo, bancos, páginas, llamadas, ISR, conflictos y memoria agotada. Probar blink, ADC, UART, PWM, I2C y SPI tanto en simulador como en placa real. Validar temporizaciones con instrumentos. Revisar configuración de reloj, fuses y condiciones eléctricas.

15. ESTADO Y DECISIONES PENDIENTES
BASM es una propuesta de diseño; las instrucciones extendidas, API, herramientas y hojas de ruta no se han implementado ni validado en este documento. Falta formalizar gramática, ABI, alcance de alias, semántica de desbordamiento, modelo de interrupciones, convención de llamadas, formato de objetos, licencias y versiones. Los ejemplos son conceptuales salvo el ejemplo de Assembly convencional, que igualmente requiere pruebas. La nomenclatura BASM reemplaza la propuesta anterior PASM (PyPIC Assembly+).

Principio rector: la simplicidad de BASIC, el poder de Assembly y el rendimiento del código máquina nativo. Facilitar el acceso al hardware sin restringirlo.
