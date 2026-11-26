# SDK: temporizadores (`pypic.periph.timer`)

Timer0, Timer1 y Timer2 son **periféricos**: se importan como nombres, igual que los pines, y el compilador los valida contra el Device Pack
(`peripherals.json`). Importar uno que el dispositivo no trae es el error `PYP2120`. Todo se resuelve al compilar; el MCU ejecuta solo instrucciones nativas.

```python
from pypic.device import PIC16F887
from pypic.periph.timer import TMR0, TMR1, TMR2, INTERNAL, T1OSC
```

Los parámetros de `start(...)` son los de los registros (`OPTION_REG`, `T1CON`, `T2CON`, `PR2`), todos constantes: el principio es el del ensamblador,
con el mismo control y el mismo costo en ciclos.

## Métodos

| Método | TMR0 | TMR1 | TMR2 | Qué hace |
|---|---|---|---|---|
| `start(...)` | ✓ | ✓ | ✓ | Configura y arranca (argumentos con nombre, ver abajo) |
| `every_us(n)` / `every_ms(n)` | – | – | ✓ | Azúcar: resuelve en compilación `prescaler × (PR2+1) × postscaler` **exactos** |
| `stop()` | – | ✓ | ✓ | Detiene (`TMR1ON`/`TMR2ON`). Timer0 no se puede detener en hardware |
| `value()` | uint8 | uint16 | uint8 | Lee el contador (Timer1: lectura coherente H-L-H, DS40001291H 6.5.1) |
| `set(v)` | ✓ | ✓ | ✓ | Precarga el contador |
| `expired()` | ✓ | ✓ | ✓ | `True` si la bandera (`T0IF`, `TMR1IF`, `TMR2IF`) está activa |
| `clear()` | ✓ | ✓ | ✓ | Baja la bandera |
| `interrupt_on()` / `interrupt_off()` | ✓ | ✓ | ✓ | Habilita/inhabilita la interrupción (activa `PEIE` en Timer1/2) |

## Argumentos de `start`

| Timer | Argumentos |
|---|---|
| `TMR0` | `prescaler` (1, 2, 4 … 256), `source` (`INTERNAL` o `T0CKI`), `edge` (`RISING` o `FALLING`, solo con `T0CKI`) |
| `TMR1` | `prescaler` (1, 2, 4, 8), `source` (`INTERNAL`, `T1CKI`, `T1OSC`), `sync` (solo con reloj externo), `gate` (`T1G_HIGH`, `T1G_LOW`, `C2_HIGH`, `C2_LOW`) |
| `TMR2` | `prescaler` (1, 4, 16), `period` (valor de `PR2`, 0..255), `postscaler` (1..16) |

Un valor que el hardware no tiene (`prescaler=3`, `period=300`) es un error que lista los valores válidos.
Los pines que reclama un reloj externo (`RA4`/`T0CKI`, `RC0`/`T1CKI`, `RC0`–`RC1`/`T1OSC`, `RB5`/`T1G`) se registran para detectar conflictos con el GPIO.

## Periodos exactos: `every_us` y `every_ms`

El periodo de la bandera de Timer2 es `prescaler × (PR2 + 1) × postscaler` ciclos de instrucción. `every_ms(1)` a 4 MHz son 1000 ciclos y se resuelve
a `prescaler=1, period=249, postscaler=4`. Si el periodo pedido no se puede dar **exactamente** (el máximo es 65 536 ciclos, o el número no se
descompone) es un error `PYP2126` que indica los periodos exactos más cercanos; entonces se usa `start(...)` con valores elegidos.

```python
TMR2.every_ms(20)          # 20 000 ciclos a 4 MHz: prescaler 16, PR2 = 124, postscaler 10
```

## Ejemplo

`examples/python/temporizadores/timer_blink.py` parpadea RD0 cada 500 ms contando 25 avisos de Timer2, sin esperas bloqueantes.

## Verificación

El simulador (`pypic-pic14::sim`) modela Timer0/1/2 con reloj interno y registra el ciclo exacto de cada bandera (`Sim::timer_events`).
`crates/pypic-pic14/tests/timer.rs` comprueba que cada periodo es exacto: Timer2 con prescaler/PR2/postscaler, Timer0 con sus 9 preescaladores,
Timer1 con sus 4, `every_ms`/`every_us` a 4 y 20 MHz, la precarga y `stop`.

**Límites del modelo:** no simula relojes externos (`T0CKI`, `T1CKI`, `T1OSC`), la compuerta de Timer1 ni el modo asíncrono; esas rutas solo se
verifican por el valor de los registros que se escriben.
