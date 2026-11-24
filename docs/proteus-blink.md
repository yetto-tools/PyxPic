# Blink de muestra para Proteus (`examples/hex/blink_rd0.hex`)

**Estado: verificado solo en el simulador del host (`pypic-pic14`), NO en Proteus todavía** (issue de seguimiento en GitHub).
Es un programa hecho a mano con el codificador de PyPIC (`crates/pypic-pic14/src/samples.rs`), no salida del compilador.

## Montaje en Proteus

1. Un **PIC16F887** (PDIP-40).
2. Un **LED con una resistencia de 330 Ω** entre **RD0 (pin 19)** y GND. LED encendido = RD0 en alto.
3. En las propiedades del PIC:
   - **Program File:** `examples/hex/blink_rd0.hex`.
   - **Processor Clock Frequency:** `4MHz`. Proteus usa esta propiedad para el tiempo; el retardo del programa está calculado para un ciclo de instrucción de 1 MHz (Fosc = 4 MHz).
4. No hace falta conectar MCLR, ni cristal ni alimentación explícita: las palabras de configuración usan el oscilador interno (INTOSCIO) y MCLR interno (MCLRE = 0).

## Qué debería pasar

RD0 arranca como salida en bajo y parpadea: **0,59 s encendido y 0,59 s apagado** (período de 1,18 s).
Si ves otra velocidad, comprueba primero la propiedad *Processor Clock Frequency*: el período escala con ella.

## Qué comprobó el simulador del host (ciclo exacto, DS40001291H)

| Medida | Ciclos |
|---|---|
| Alto → bajo | 592 143 |
| Bajo → alto | 592 145 |
| Período | 1 184 288 (= 1,184 s a 1 MHz) |

## Palabras de configuración (`2007h` → byte `400Eh`, `2008h` → `4010h`)

| CONFIG1 = `20D4h` | |
|---|---|
| FOSC = 100 | INTOSCIO (oscilador interno, RA6/RA7 como E/S) |
| WDTE = 0 | WDT desactivado |
| PWRTE = 1 | PWRT desactivado |
| MCLRE = 0 | RE3 es entrada; MCLR interno a VDD |
| CP = 1, CPD = 1 | sin protección de código ni de datos |
| BOREN = 00 | BOR desactivado |
| IESO = 0, FCMEN = 0 | sin cambio ni monitor de reloj |
| LVP = 0 | RB3 es E/S digital |
| DEBUG = 1 | depurador desactivado |

CONFIG2 = `3FFFh` (sin protección de escritura, BOR a 4,0 V).

## Si algo no funciona

`cargo run -p pypic-cli -- disasm examples/hex/blink_rd0.hex` muestra exactamente qué contiene el archivo.
Cuéntame qué hace el LED y qué ves en las propiedades del PIC (o una captura) y lo reviso.
