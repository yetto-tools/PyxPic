# SDK: PWM de los módulos CCP (`pypic.periph.pwm`)

**Estado: implementado (segundo módulo de #45, solo el modo PWM de una salida).** `PWM1` es el módulo CCP1 (pin `RC2`/`P1A`/`CCP1`) y `PWM2` es CCP2 (pin `RC1`/`CCP2`) del PIC16F887 (DS40001291H, cap. 11, pp. 122-129). Captura, comparación y los modos de puente (medio puente, puente completo, dirección de pulso) **no** están: ver *Lo que no hay todavía*.

```python
from pypic.device import PIC16F887
from pypic.periph.pwm import PWM1, PWM2

CLOCK: Internal[4 * MHz]

PWM1.start(frequency=1000)       # periodo exacto de 1 kHz; el ciclo de trabajo empieza en 0
PWM1.duty(250)                   # 250 de 1000 cuentas: 25 %
```

## Qué es el ciclo de trabajo: cuentas, no porcentajes

El datasheet define el ciclo de trabajo como un número de 10 bits (`CCPRxL:CCPxCON<5:4>`) que se mide en **cuentas de `TOSC × prescaler`** (ecuación 11-2, p.127). Una cuenta es una unidad de tiempo; el periodo completo tiene **`4 × (PR2 + 1)` cuentas** (ecuaciones 11-1 y 11-3). PyPIC no escala ni multiplica por ti: `PWM1.duty(v)` escribe `v` tal cual.

| Periodo pedido (a 4 MHz) | Prescaler | `PR2` | Cuentas por periodo (`full_scale`) | Resolución |
|---|---|---|---|---|
| 1 kHz (1000 ciclos) | 4 | 249 | **1000** | ≈ 10 bits |
| 20 kHz (50 ciclos) | 1 | 49 | **200** | ≈ 7,6 bits |
| prescaler 1, `period=255` (256 ciclos, 3906,25 Hz) | 1 | 255 | **1024** | 10 bits |

- `duty(0)`: la salida **nunca sube** (p.126). `duty(v)` con `v` igual o mayor que `full_scale`: la salida queda **siempre en alto** (nota de la p.128: con un ancho mayor que el periodo el pin no cambia). Una constante mayor que `full_scale` es el aviso `PYP3015`.
- El valor tiene 10 bits: `0..=1023`. Una constante fuera de rango es `PYP2202`; con una variable, **los bits sobre el 10 se ignoran** (`1027` es `3`), igual que en el hardware.
- Con `period=255` y prescaler 1 el periodo tiene exactamente **1024 cuentas**: una lectura del ADC (0..1023) es un ciclo de trabajo sin escalar (`examples/python/pwm/pwm.py`).

## Operaciones

| Operación | Qué hace | Costo |
|---|---|---|
| `PWM1.start(frequency=1000)` | Configura Timer2 y el módulo y enciende la salida (orden de 11.5.7, abajo) | hasta **un periodo** de espera (el primer desbordamiento de Timer2) más la configuración |
| `PWM1.start(frequency=1000, duty=250)` | Igual, con un ciclo de trabajo inicial constante (0 a 1023; 0 por defecto) | igual |
| `PWM1.start(prescaler=4, period=249)` | El periodo a mano: `prescaler` 1, 4 o 16 y `period` = valor de `PR2` (0 a 255). Periodo = `prescaler × (PR2 + 1)` ciclos | igual |
| `PWM1.duty(v)` | `CCPRxL = v >> 2` y `DCxB = v & 3` sin tocar el modo | **6 ciclos** con una constante; **27** con una variable (no depende del valor) |
| `PWM1.stop()` | `CCPxCON = 0`: el módulo se apaga y el pin vuelve al puerto | **1 ciclo** |

Una sola forma por operación: o `frequency` o `prescaler` + `period` (`PYP2201` si se mezclan o falta una).

### `frequency=`: periodos exactos o error

El periodo es `prescaler × (PR2 + 1)` ciclos de instrucción, con prescaler 1, 4 o 16 y `PR2 + 1` de 1 a 256: de 1 a 4096 ciclos. PyPIC elige el **prescaler más chico** posible (la mejor resolución). Si la frecuencia no cae en un número entero de ciclos que Timer2 pueda dar, **no redondea**: es el error `PYP2207`, con las dos frecuencias exactas más cercanas.

```
PWM1.start(frequency=3000)   # a 4 MHz: 333,33 ciclos
  error[PYP2207]: 3000 Hz no cae en un periodo exacto de Timer2 a 4000000 Hz ...; las frecuencias exactas mas cercanas son ...
```

La misma regla de periodos exactos que `TMR2.every_ms` (docs/sdk-timer.md). Con `prescaler=`/`period=` el periodo que declaras es el que sale; el simulador lo mide en las pruebas contra las tablas 11-3 y 11-4 del datasheet.

### El arranque, paso a paso (DS40001291H 11.5.7, p.129)

`start` hace los seis pasos del datasheet, en este orden:

1. `TRISC<pin> = 1` (el pin como entrada mientras se configura);
2. `PR2 = periodo`;
3. y 4. `CCPRxL` y `CCPxCON` con el modo PWM (`CCPxM = 1100`, `P1M = 00` en CCP1) y el ciclo de trabajo inicial;
5. `TMR2IF = 0` y `T2CON = TMR2ON | prescaler` (postscaler 1:1, que no afecta al PWM, p.126);
6. espera `TMR2IF` (un periodo nuevo empezó) y entonces `TRISC<pin> = 0` (salida) y `TMR2IF = 0`.

El paso 6 evita un primer pulso incompleto: por eso **`start` bloquea hasta un periodo** y no debe llamarse desde una interrupción. Si Timer2 **ya corre** (el otro módulo PWM ya arrancó), el paso 5 no reescribe `T2CON`: escribirlo borra el preescalador y alargaría un periodo del otro módulo. El segundo `start` espera igualmente un periodo nuevo.

## Un periodo para los dos módulos

`PWM1` y `PWM2` comparten **Timer2** (tabla 11-2, p.123): el programa tiene **un solo periodo**. Dos `start` con periodos distintos son `PYP2208`. Usar `TMR2` y un PWM en el mismo programa es `PYP2206` (el PWM es dueño de `T2CON` y `PR2`).

## Pines

`PWM1` toma **`RC2`** y `PWM2` toma **`RC1`** (de `pins.json` del Device Pack). Importar el pin también como E/S (`from pypic.periph.gpio import RC2`) es `PYP2150`, el mismo mecanismo que con el ADC o el oscilador. Tras `stop()` el pin vuelve a ser del puerto y, como el arranque lo dejó como salida, **conserva el nivel de su latch** (no queda en alto impedancia).

## Doble búfer y lo que NO es atómico

`CCPRxL` y `DCxB` se pueden escribir en cualquier momento, pero el valor pasa a `CCPRxH` **al terminar el periodo** (p.127): no hay pulsos cortados. Sin embargo `duty(v)` escribe `CCPRxL` y luego los dos bits de `CCPxCON` en dos pasos (primero se ponen a 0 y luego se les hace OR con el valor): si el periodo termina **justo entre esas dos instrucciones (una ventana de 2 ciclos)**, ese periodo sale con la parte fraccionaria en 0 (un error de a lo más 3 cuentas, menos de un escalón de `CCPRxL`). Y si cambia a la vez el byte alto y el bajo, un periodo puede salir con el byte alto nuevo y el bajo viejo, o al revés. Es un límite del hardware (los dos campos viven en registros distintos) y del código de dos pasos.

## Reglas

| Código | Regla |
|---|---|
| `PYP2200` | `pypic.periph.pwm` no exporta ese nombre (`PWM1`, `PWM2`) o el dispositivo no trae el módulo (`ECCP`/`CCP2` de `peripherals.json`) |
| `PYP2201` | Argumentos mal formados de `start` (sin nombre, sin periodo, mezclados, faltan `prescaler`/`period`, argumento desconocido) o de otro método |
| `PYP2202` | Valor fuera de rango: `prescaler` no es 1/4/16, `period` fuera de 0-255, `frequency` fuera de 1 a 4 000 000, ciclo de trabajo fuera de 0-1023 |
| `PYP2203` | Método que no existe (`start`, `duty`, `stop`) |
| `PYP2204` | Se usa el módulo pero ningún `start()` es alcanzable desde `main` o `@init_device` (se comprueba que exista, **no el orden**) |
| `PYP2205` | `PWM1` o `PWM2` usado como valor |
| `PYP2206` | El programa usa Timer2 (`TMR2`) y el PWM a la vez |
| `PYP2207` | `frequency=` no cae en un periodo exacto (muestra las dos más cercanas) |
| `PYP2208` | Los dos módulos piden periodos distintos |
| `PYP2150` | El pin del módulo también se importa como E/S |
| `PYP3015` | Ciclo de trabajo constante mayor que el periodo: la salida queda siempre en alto |

**Límite de la comprobación de `start()`**: como en el ADC, se verifica que **exista** un `start()` alcanzable, no el orden. `duty` antes de `start` compila; el `start` posterior vuelve a escribir `CCPRxL` y `CCPxCON` con su `duty=` inicial.

## Verificación

El simulador del núcleo modela el PWM: **al empezar cada periodo** (cuando `TMR2 = PR2`) engancha `CCPRxL:DCxB` (doble búfer) y guarda el ciclo exacto y el valor en `Sim::pwm_events`; el nivel del pin (`PORTC`) se calcula **una vez por ciclo de instrucción** como "posición del periodo menor que el valor enganchado" y solo se escribe si `TRISC` deja el pin como salida. **No modela el flanco dentro del ciclo de instrucción** (con prescaler 1 una cuenta es un cuarto de ciclo): el ancho del pulso en el simulador es exacto en cuentas pero se observa con resolución de un ciclo.

`crates/pypic-pic14/tests/pwm.rs` (18 pruebas) comprueba: los registros que deja el arranque (`PR2`, `T2CON`, `CCPxCON`, `TRISC`); que la salida se habilita después del primer inicio de periodo; **periodos exactos** de las tablas 11-3 y 11-4 (8 y 20 MHz); la elección del prescaler más chico; el error de frecuencia no exacta con sus vecinas; el doble búfer (el valor nuevo se engancha solo al inicio de periodo); el **ancho del pulso en cuentas** para 0, 1, 250, 500, 999 y 1000; `duty(0)` (nunca sube) y `duty` mayor que el periodo (siempre alto); el ciclo de trabajo **variable contra el intérprete de referencia** con 19 valores (incluidos 1024, 1027 y 65535: se ignoran los bits sobre el 10); dos módulos compartiendo Timer2 sin alargar un periodo; `stop`, y que `duty` después de `stop` no vuelve a poner el modo PWM; todas las reglas de arriba; los costos; y `examples/python/pwm/pwm.py` (potenciómetro → brillo: una lectura del ADC es un ciclo de trabajo).

**Qué no demuestra:** que el simulador y el compilador usen el mismo modelo del hardware no prueba el pin real. **No se ha probado en Proteus ni con un LED, un motor o un osciloscopio**; el periodo, el ancho y el orden de arranque están verificados contra las ecuaciones y los pasos del datasheet y contra el simulador, no contra un chip.

## Lo que no hay todavía

- **Captura y comparación** de CCP (Timer1): una entrega aparte con su propia API.
- **ECCP (PWM mejorado)**: medio puente con banda muerta, puente completo adelante y atrás, dirección de pulso (`PSTRCON`), reinicio automático y apagado automático (`ECCPAS`, `PWM1CON`). `PWM1` usa solo la salida P1A (`P1M = 00`).
- **Polaridad activa en bajo** (`CCPxM = 1101`..`1111`).
- **Interrupción** del PWM y salida alternativa de CCP2 (el PIC16F887 de este Device Pack solo tiene CCP2 en `RC1`).
- Escalar un porcentaje o un voltaje: hoy `duty` toma cuentas; el cálculo (`v * full_scale / 100`) lo escribe el programa, con el costo de la multiplicación a la vista.
