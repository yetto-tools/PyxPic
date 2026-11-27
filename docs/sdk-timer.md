# SDK: temporizadores (`pyxpic.periph.timer`)

Timer0, Timer1 y Timer2 son **periféricos**: se importan como nombres, igual que los pines, y el compilador los valida contra el Device Pack
(`peripherals.json`). Importar uno que el dispositivo no trae es el error `PYP2120`. Todo se resuelve al compilar; el MCU ejecuta solo instrucciones nativas.

```python
from pyxpic.device import PIC16F887
from pyxpic.periph.timer import TMR0, TMR1, TMR2, INTERNAL, T1OSC
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

### Periodos más largos que Timer2: el contador de software

Timer2 es un contador de **8 bits**: cuenta hasta `PR2`, se desborda (vuelve a 0 y levanta su bandera) y empieza de nuevo. Con el prescaler y el
postscaler más grandes tarda como máximo **65 536 ciclos** en desbordarse: 32,768 ms a 8 MHz. Un `every_ms(500)` a 8 MHz son 1 000 000 ciclos y no cabe.

BASIC no se queda corto frente a lo que se haría a mano en C o ensamblador (un tick del hardware y un contador): **el compilador lo hace solo**.
Parte el periodo en un **tick exacto** de Timer2 y N vueltas contadas por software (`40 000 ciclos × 25` en el ejemplo: 20 ms × 25 = 500 ms), y avisa
con `PYP3026` de lo que hizo y por qué. `TMR2.expired` consume cada tick del hardware, cuenta y es verdadero en la última vuelta (y se queda así hasta
`TMR2.clear`, como la bandera del hardware); `TMR2.clear` **reinicia el periodo** (bandera del hardware, cuenta y bandera de software).

```basic
TMR2.every_ms 500          ' a 8 MHz: tick de 20 ms x 25 vueltas
main:
  IF TMR2.expired THEN
    TMR2.clear
    TOGGLE RD0
  ENDIF
  GOTO main
```

- **Costo:** unos 5 bytes de RAM (contador de 16 bits, bandera de 1 y dos temporales) y unas 30 instrucciones por consulta de `expired`. Un periodo que
  cabe en Timer2 no gasta nada de esto.
- **Elige el tick más largo** que Timer2 da exacto (menos vueltas de software). La cuenta es de 16 bits: hasta 65 535 vueltas.
- **Sin interrupción:** `TMR2.interrupt_on` con un periodo así es un error (`PYP2127`): la interrupción de Timer2 salta en cada tick, no en el periodo completo.
- **Si no se puede formar** (por ejemplo `every_us(100003)` a 8 MHz son 200 006 ciclos, que ni un tick exacto por un contador forma) es el error `PYP2126` con
  **causa** (el desbordamiento del contador y por qué ese número no se forma) y **solución** (los periodos exactos más cercanos: 100,002 ms y 100,004 ms).
- **A mano en ensamblador** también se puede, dentro del propio BASIC, con un bloque `ASM` (hay una prueba que mide 500,00 ms): el contador queda a tu cargo.

```asm
tick500:                      ; GOSUB tick500 -> b3 = 1 cada 500 ms (tick de 20 ms de TMR2.every_ms 20)
    CLRF b3
    BTFSS PIR1,1              ; TMR2IF
    RETURN
    BCF PIR1,1
    INCF b2,F
    MOVLW 25                  ; 25 ticks x 20 ms
    XORWF b2,W
    BTFSS STATUS,2            ; Z
    RETURN
    CLRF b2
    INCF b3,F
    RETURN
```

## Ejemplo

`examples/python/temporizadores/timer_blink.py` parpadea RD0 cada 500 ms contando 25 avisos de Timer2, sin esperas bloqueantes.

## Verificación

El simulador (`pyxpic-pic14::sim`) modela Timer0/1/2 con reloj interno y registra el ciclo exacto de cada bandera (`Sim::timer_events`).
`crates/pyxpic-pic14/tests/timer_long.rs` mide en el simulador que un periodo de 500 ms a 8 MHz cambia RD0 cada 500,00 ms (en BASIC, en Python y con el bloque `ASM`), y prueba el error con causa y solución y el aviso.
`crates/pyxpic-pic14/tests/timer.rs` comprueba que cada periodo es exacto: Timer2 con prescaler/PR2/postscaler, Timer0 con sus 9 preescaladores,
Timer1 con sus 4, `every_ms`/`every_us` a 4 y 20 MHz, la precarga y `stop`.

**Límites del modelo:** no simula relojes externos (`T0CKI`, `T1CKI`, `T1OSC`), la compuerta de Timer1 ni el modo asíncrono; esas rutas solo se
verifican por el valor de los registros que se escriben.
