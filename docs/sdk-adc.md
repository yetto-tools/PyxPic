# SDK: convertidor analógico-digital (`pypic.periph.adc`)

**Estado: implementado (primer módulo de #45).** Convertidor de 10 bits con 14 canales del PIC16F887 (DS40001291H, cap. 9). Todo se resuelve al compilar; el costo en ciclos está a la vista.

```python
from pypic.periph.adc import ADC, AN0, AN5

CLOCK_HZ: Const[u32] = 4_000_000

ADC.start()                      # ADCON1/ADCON0: resultado a la derecha, VDD/VSS, reloj de conversión elegido, ADC encendido
v: u16 = ADC.read(AN0)           # 0..1023
```

## Operaciones

| Operación | Qué hace | Ciclos |
|---|---|---|
| `ADC.start()` / `ADC.start(clock=FOSC_8)` | Configura y enciende. Sin `clock` lo elige el compilador (abajo) | 6 |
| `ADC.select(AN5)` | Pone el canal en `CHS`, el pin como **entrada analógica** (`TRIS` = 1, `ANSEL` = 1); no toca el reloj, GO ni ADON | 12 |
| `ADC.go()` | `GO/DONE = 1` | 1 |
| `ADC.done()` → `bool` | La conversión terminó | 3 |
| `ADC.result()` → `u16` | `ADRESH:ADRESL` justificado a la derecha | 6 |
| `ADC.read(AN5)` → `u16` | `select` + espera de adquisición de 5 µs + `go` + espera de `done` + `result` | 29 / 50 / 121 / 136 a 1 / 4 / 8 / 20 MHz |

`read` **es exactamente esa secuencia**, no un sinónimo: las primitivas dan el control del ensamblador (arrancar una conversión y hacer otra cosa mientras tanto, o esperar un tiempo propio) y `read` es la
forma corta. Todo con una sola forma por operación: no hay `read10` ni una versión de 8 bits; para un byte, `u8(ADC.read(AN0) >> 2)`.

## Lo que el compilador decide por ti (y dónde se ve)

- **Reloj de conversión** (`ADCS`): el **más rápido** de `FOSC/2`, `FOSC/8`, `FOSC/32` cuyo `TAD` cae en **1,6 a 6,0 µs** (AD130, p.15244); si ninguno, el **RC interno** del ADC. Depende de `CLOCK_HZ`:
  1 MHz → `FOSC/2` (2 µs) · 4 MHz → `FOSC/8` (2 µs) · 8 MHz → `FOSC/32` (4 µs) · 20 MHz → `FOSC/32` (1,6 µs) · 32 kHz → RC. Un `clock=` a mano que se salga del rango es el aviso **`PYP3012`**.
- **Adquisición**: tras cambiar de canal, `read` espera **5 µs** (AD132, ecuación 9-1: 4,67 µs a 50 °C con 10 kΩ), redondeados hacia arriba a ciclos de instrucción, con la misma espera exacta de `delay_cycles`; sale en `--timing`.
  Si usas las primitivas, esa espera es tuya (`delay_us(5)`); el simulador la comprueba.
- **Referencias**: VDD y VSS (`VCFG` = 00). Referencias externas (`VREF+`/`VREF-`) todavía no.

## Reglas

| Código | Regla |
|---|---|
| `PYP2170` | `pypic.periph.adc` no exporta ese nombre (lista los canales), o el dispositivo no tiene ADC / ese canal |
| `PYP2171` | Se usa el ADC pero nunca se llama a `ADC.start()` |
| `PYP2172` | Argumentos de `start` inválidos (solo `clock=`) |
| `PYP2173` | Método que no existe |
| `PYP2174` | El canal debe ser una constante `AN0`..`AN13` importada |
| `PYP2175` | `ADC` o un canal usado como valor |
| `PYP2150` | El pin de un canal también se importa como E/S (`RA0` y `AN0`): lo detecta el mismo mecanismo de [pines que toma un periférico](sdk-regs.md) |
| `PYP3012` | `clock=` da un TAD fuera de 1,6 a 6,0 µs |

`ADC.read` **no se admite en una interrupción** (incluye la espera, que ocupa la CPU); `done()`, `result()`, `go()` y `select()` sí.

## Verificación

El simulador del núcleo modela el ADC: `GO/DONE`, la conversión de 11 TAD, `ADRESH:ADRESL` según `ADFM` y `ADIF`, y **vigila el datasheet**: registra una violación si el programa pone `GO` con un `TAD` fuera de
1,6–6,0 µs, con menos de 5 µs desde el último cambio de canal, o con el ADC apagado. `crates/pypic-pic14/tests/adc.rs`: lecturas contra el intérprete de referencia, el reloj automático a 1, 2, 4, 8, 16 y 20 MHz y a 32 kHz,
la adquisición a cuatro frecuencias, `GO` sin espera (detectado), conflictos de pines, reglas del lenguaje, la tabla de pines contra el Device Pack y los costos de arriba. Ejemplo: `examples/adc.py`.
**No se ha probado con un convertidor real.**
