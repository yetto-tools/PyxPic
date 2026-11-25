# SDK: convertidor analógico-digital (`pypic.periph.adc`)

**Estado: implementado (primer módulo de #45).** Convertidor de 10 bits con 14 canales del PIC16F887 (DS40001291H, cap. 9 y tabla 17-11). Todo se resuelve al compilar; el costo en ciclos está a la vista.

```python
from pypic.device import PIC16F887
from pypic.periph.adc import ADC, AN0, AN5

CLOCK: Internal[4 * MHz]

ADC.start()                      # ADCON1/ADCON0: resultado a la derecha, referencia VDD/VSS, reloj de conversión elegido, ADC encendido
v: uint16 = ADC.read(AN0)           # 0..1023
```

## Las condiciones eléctricas que PyPIC asume

El compilador **no conoce tu circuito**, así que el tiempo de adquisición y la ventana de `TAD` no son verdades universales sino **suposiciones declaradas**, con valores del datasheet (tabla 17-11, p.258) y una forma de cambiarlas:

| Parámetro | Qué dice el datasheet | Qué hace PyPIC |
|---|---|---|
| **Adquisición** `TACQ` (AD132) | No da mínimo ni máximo: solo el **valor típico de 11,5 µs** (5 V, 25 °C). Depende de la impedancia de la fuente (se recomienda ≤ 10 kΩ), la tensión y la temperatura; la ecuación 9-1 da 4,67 µs para 10 kΩ, 50 °C y 5 V, y usa `TAMP`, cuyo **máximo es 5 µs** (AD133) | Por defecto **11,5 µs**, redondeados hacia arriba a ciclos. `ADC.start(acquisition_us=N)` lo fija (1–1000). Menos de 5 µs es el aviso `PYP3013` |
| **`TAD`** reloj de FOSC (AD130) | **Mínimo 1,6 µs** con VREF ≥ 3,0 V y **3,0 µs** con VREF de rango completo; **máximo 9,0 µs** | Supone **VDD > 3,0 V y VREF = VDD**. `ADC.start(low_voltage=True)` supone VDD < 3,0 V (mínimo 3,0 µs) |
| `TAD` recomendado (tablas 9-1 y 17-18) | 1,6 a 6,0 µs con VDD > 3,0 V | El reloj automático **prefiere esa ventana** y solo sale de ella (hasta los 9,0 µs de AD130) si no hay otra opción |
| RC interno del ADC | `TAD` 1,6–6,0 µs (típico 4 µs) a 5 V y 3,0–9,0 µs (típico 6 µs) a 2,5 V | Se usa cuando ningún reloj derivado de FOSC cumple AD130 |
| Referencias | `VCFG` | Solo **VDD y VSS**; referencias externas (`VREF+`/`VREF-`) todavía no |

Si tu fuente tiene impedancia alta, el sensor está caliente o la alimentación es baja, **fija `acquisition_us`** con la ecuación 9-1 del datasheet. Un programa tiene **una sola** condición eléctrica: todas sus llamadas a `ADC.start(...)` deben pedir lo mismo (`PYP2176`).

## Operaciones

| Operación | Qué hace | Ciclos |
|---|---|---|
| `ADC.start()` / `ADC.start(clock=FOSC_8, acquisition_us=20, low_voltage=True)` | Configura y enciende. Sin `clock` lo elige el compilador | 6 |
| `ADC.select(AN5)` | Pone el canal en `CHS`, el pin como **entrada analógica** (`TRIS` = 1, `ANSEL` = 1); no toca el reloj, GO ni ADON | 12 |
| `ADC.go()` | `GO/DONE = 1` | 1 |
| `ADC.done()` → `bool` | La conversión terminó | 3 |
| `ADC.result()` → `uint16` | `ADRESH:ADRESL` justificado a la derecha | 6 |
| `ADC.read(AN5)` → `uint16` | `select` + espera de adquisición + `go` + espera de `done` + `result` | **30 / 57 / 134 / 169** a 1 / 4 / 8 / 20 MHz (con la adquisición por defecto) |

`read` **es exactamente esa secuencia**, no un sinónimo: las primitivas dan el control del ensamblador (arrancar una conversión y hacer otra cosa mientras tanto, o esperar un tiempo propio) y `read` es la forma corta. No hay `read10` ni una versión de 8 bits; para un byte, `uint8(ADC.read(AN0) >> 2)`.
Con las primitivas la espera de adquisición es tuya (`delay_us(12)` basta con la adquisición por defecto); el simulador la vigila.

## Reloj de conversión automático

`ADCS` es el **más rápido** de `FOSC/2`, `FOSC/8`, `FOSC/32` con `TAD` en la ventana recomendada; si no hay, el más rápido que cumple AD130; si tampoco, el RC interno. Con `low_voltage=False`:
1 MHz → `FOSC/2` (2 µs) · 4 MHz → `FOSC/8` (2 µs) · 8 MHz → `FOSC/32` (4 µs) · 20 MHz → `FOSC/32` (1,6 µs) · 250 kHz → `FOSC/2` (8 µs, fuera de lo recomendado pero válido) · 32 kHz → RC.
Con `low_voltage=True`: 4 MHz → `FOSC/32` (8 µs) y 1 MHz → `FOSC/8` (8 µs). Un `clock=` a mano que se salga de AD130 es el aviso **`PYP3012`**; entre 6 y 9 µs **no** avisa.

## Reglas

| Código | Regla |
|---|---|
| `PYP2170` | `pypic.periph.adc` no exporta ese nombre (lista los canales), o el dispositivo no tiene ADC / ese canal |
| `PYP2171` | Se usa el ADC pero no hay un `ADC.start()` que `main` o `@init_device` puedan ejecutar (una llamada en una función que nadie llama no cuenta) |
| `PYP2172` | Argumentos de `start` inválidos (admite `clock`, `acquisition_us`, `low_voltage`) |
| `PYP2173` | Método que no existe |
| `PYP2174` | El canal debe ser una constante `AN0`..`AN13` importada |
| `PYP2175` | `ADC` o un canal usado como valor |
| `PYP2176` | Las llamadas a `ADC.start()` piden condiciones distintas |
| `PYP2150` | El pin de un canal también se importa como E/S (`RA0` y `AN0`): lo detecta el mismo mecanismo de [pines que toma un periférico](sdk-regs.md) |
| `PYP3012` | `clock=` da un `TAD` fuera de AD130 |
| `PYP3013` | `acquisition_us` menor que el `TAMP` máximo (5 µs) |

**Límite de la comprobación de `start()`**: el compilador verifica que **exista** un `ADC.start()` alcanzable desde `main` o `@init_device`, **no el orden** en que se ejecuta respecto a las lecturas. Leer antes de iniciar compila; en el chip la conversión nunca termina (con `ADON` apagado `GO/DONE` no se borra) y el simulador lo registra.
`ADC.read` **no se admite en una interrupción** (incluye la espera, que ocupa la CPU); `done()`, `result()`, `go()` y `select()` sí.

## Verificación

El simulador del núcleo modela el ADC: `GO/DONE`, la conversión de 11 `TAD`, `ADRESH:ADRESL` según `ADFM` y `ADIF`, y **vigila el contrato**: una violación si se pone `GO` con un `TAD` fuera de AD130 (con la alimentación que se le declare, `adc_low_voltage`), con menos adquisición que la declarada (`adc_tacq_ns`, por defecto 11,5 µs) desde el último cambio de canal, o con el ADC apagado.
**Que el simulador no se queje no demuestra que el circuito real cumpla**: el simulador usa las mismas suposiciones que el compilador, de modo que una adquisición insuficiente para *tu* fuente (alta impedancia, temperatura) no se ve en él: se calcula con la ecuación 9-1 y se declara con `acquisition_us`.
`crates/pypic-pic14/tests/adc.rs`: lecturas contra el intérprete de referencia, reloj automático desde 32 kHz hasta 20 MHz con VDD alta y baja, adquisición configurable, `TAD` entre 6 y 9 µs sin aviso y por encima de 9 µs con él, `start()` inalcanzable, orden de lectura/inicio, conflictos de pines, reglas del lenguaje, la tabla de pines contra el Device Pack y los costos de arriba. Ejemplo: `examples/adc.py`.
**No se ha probado con un convertidor real.**
