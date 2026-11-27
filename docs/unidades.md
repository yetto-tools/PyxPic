# Unidades en los parámetros del SDK

**Estado: implementado (fase 4 de #69).** Los literales con unidad (`docs/sdk-reloj.md`) se usan en los parámetros de los periféricos. **Toda la conversión la hace el compilador**: el firmware generado es idéntico al de la forma con enteros de siempre (hay una prueba por parámetro que compara las palabras de programa), sin RAM, sin instrucciones y sin coma flotante.

| Llamada | Magnitud | Unidad del parámetro | Equivale a |
|---|---|---|---|
| `UART.start(baud=9600baud)` | baudios | baud | `UART.start(baud=9600)` (también `9.6kbaud`) |
| `SPI.start(frequency=1MHz)` | frecuencia | Hz | `SPI.start(freq=1000000)` |
| `I2C.start(frequency=100kHz)` | frecuencia | Hz | `I2C.start(freq=100000)` |
| `PWM1.start(frequency=20kHz, duty=50pct)` | frecuencia, porcentaje | Hz; cuentas del periodo | `PWM1.start(frequency=20000, duty=100)` a 4 MHz |
| `TMR2.every(20ms)` | tiempo | ciclos | `TMR2.start(prescaler=…, period=…, postscaler=…)` resuelto |
| `pause(500ms)`, `wait(2s)`, `delay_us(250us)` | tiempo | ms, s, µs | `pause(500)`, `wait(2)`, `delay_us(250)` |
| `ADC.start(acquisition_us=20us)` | tiempo | µs | `acquisition_us=20` |
| `ADC.start(reference=5V)` | tensión | µV | decide `low_voltage` |

**Compatibilidad:** un número sin unidad sigue valiendo en todos esos parámetros (`SPI.start(freq=1000000)`, `pause(500)`). `frequency` es el nombre de SPI e I²C (como ya era el de PWM); `freq` se sigue aceptando.

## BASIC

El lexer de BASIC usa el mismo escáner que Python (`units::scan`): `8MHZ`, `11.0592MHz`, `500ms`, `3.3V` son literales con unidad, y como **BASIC no distingue mayúsculas, los sufijos tampoco** (`8MHZ`, `32.768khz`, `500MS`, `3.3v`, `50PCT`). Se busca primero la forma exacta y, si no existe, la única unidad que coincida sin importar el caso; `h` y `H` son **horas** (el henrio se escribe `henry`, o `nH`, `uH`, `mH`), así que no queda ningún par ambiguo. Python sigue distinguiendo mayúsculas. Los literales llegan al mismo AST, así que las reglas y los errores (`PYP2240`…) son los de Python: `PAUSE 500ms` genera lo mismo que `PAUSE 500`, y `BAUD 9600baud` vale. El reloj y sus ajustes tienen su forma en BASIC (`CLOCK CRYSTAL 11.0592MHZ MODE HS`, `PLL 4`, `CLOCKDIVIDER 2`, `CLOCKOUTPUT ON`, `FAILSAFE ON`, `CLOCKSWITCHING ON`, ver `docs/basic-picaxe.md`) y producen el mismo reloj que su equivalente en Python (hay una prueba que lo compara para cada forma).

## Reglas

- **Magnitud equivocada:** `UART.start(baud=9600Hz)`, `pause(500Hz)`, `I2C.start(frequency=3V)` son el error `PYP2240` («espera baudios / tiempo / frecuencia»). `delay_cycles(5ms)` también: recibe ciclos.
- **Valor no entero en la unidad del parámetro:** `pause(1500us)` o `pause(0.0005s)` son `PYP2241` (`pause` recibe milisegundos enteros): usa `delay_us(1500us)`. En cambio `pause(0.5s)` vale: son exactamente 500 ms. No se redondea nunca en silencio.
- **Porcentaje del ciclo de trabajo:** `duty=50pct` es una **parte del periodo**, no de 1023: se resuelve con PR2, que depende del reloj y de `frequency` (`cuentas = 4 × (PR2 + 1) × pct`, redondeado a la cuenta más cercana). Va de `0pct` a `100pct` (`PYP2241` fuera de ahí). `PWM.duty(…)` en ejecución sigue recibiendo cuentas.
- **`TMR2.every(20ms)`** convierte el tiempo en ciclos de instrucción con el reloj declarado y resuelve prescaler × (PR2 + 1) × postscaler con el resolvedor de siempre (el menor prescaler y postscaler que lo forman exactamente). Si el tiempo no cae en un número entero de ciclos, o Timer2 no lo alcanza (máximo 65 536 ciclos), es `PYP2243` y el mensaje da los periodos más cercanos. Solo Timer2 tiene periodo automático: `TMR0.every` y `TMR1.every` no existen. Necesita el reloj declarado.
- **`ADC.start(reference=5V)`** no hace que el PIC genere nada: es la tensión de alimentación (VDD) y solo decide `low_voltage` (menos de 3,0 V). Se valida contra la alimentación del dispositivo (`vdd_min_uv`/`vdd_max_uv` en `device.json`; PIC16F887: 2,0 V a 5,5 V, DS40001291H p.1; `PYP2242` fuera de ahí) y contradice a `low_voltage=` si los dos están y no coinciden (`PYP2172`).
- **`delay(500ms)` no existe:** se mantiene `pause` (una sola forma por operación, `docs/principios-de-diseno.md`); `pause(500ms)` ya lleva la unidad.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP2240` | La magnitud no corresponde al parámetro (o falta la unidad en `reference`, `TMR2.every`) |
| `PYP2241` | El valor no es entero en la unidad del parámetro, o el porcentaje está fuera de 0–100 |
| `PYP2242` | `reference` fuera de la alimentación del dispositivo |
| `PYP2243` | `TMR2.every`: el tiempo no es un número entero de ciclos, Timer2 no lo alcanza, o falta el reloj |

**Verificación:** `crates/pyxpic-pic14/tests/units.rs` (cada forma con unidad genera las mismas palabras de programa que su equivalente entero; todos los rechazos con su código). **No probado en hardware.**
