# SDK: comparadores y referencia de tensión (`pyxpic.periph.comparator` y `pyxpic.periph.vref`)

**Estado: implementado (cuarto módulo de #45, sin salida a pin, sincronización con Timer1 ni pestillo SR).** Los dos comparadores analógicos C1 y C2 y la referencia CVREF del PIC16F887 (DS40001291H, cap. 8, pp. 85-98; especificaciones en las tablas 17-7 y 17-8, p.256).

```python
from pyxpic.device import PIC16F887
from pyxpic.periph.comparator import CMP1, C12IN0, REF_CVREF
from pyxpic.periph.vref import CVREF

CLOCK: Internal[4 * MHz]

CVREF.start(step=8)                                    # CVREF = VDD/4 + 8/32 x VDD = VDD/2
CMP1.start(negative=C12IN0, positive=REF_CVREF)        # ¿está RA0 por debajo de la mitad de VDD?
alto: bool = CMP1.out()                                # True cuando V(+) > V(-)
```

## Comparadores

| Operación | Qué hace |
|---|---|
| `CMP1.start(negative=..., positive=..., invert=False)` | Pone las entradas como analógicas (`TRIS` y `ANSEL`), elige la referencia, escribe `CM1CON0` con `C1ON`, **espera el cambio de modo (10 µs)**, lee `CM1CON0` y baja `C1IF` |
| `CMP1.out()` → `bool` | El nivel de `C1OUT` (con la polaridad aplicada) |
| `CMP1.stop()` | `C1ON = 0` (el resto de la configuración queda) |

`CMP2` es igual con `CM2CON0`. Las dos llamadas a `start` van **solo con nombre** y `negative` y `positive` son obligatorios (el compilador escribe los registros, no hay un valor por defecto que adivinar).

| `negative=` (entrada −, `CxCH<1:0>`) | Pin | Canal |
|---|---|---|
| `C12IN0` | RA0 | AN0 |
| `C12IN1` | RA1 | AN1 |
| `C12IN2` | RB3 | AN9 |
| `C12IN3` | RB1 | AN10 |

| `positive=` (entrada +) | Qué es | Registros |
|---|---|---|
| `REF_PIN` | el pin `CxIN+`: **RA3** (AN3) en C1, **RA2** (AN2) en C2 | `CxR = 0` |
| `REF_CVREF` | la referencia CVREF (iníciala con `CVREF.start`) | `CxR = 1`, `CxRSEL = 1` |
| `REF_FIXED` | la referencia fija de **0,6 V** (0,5 a 0,7 V, VR01) | `CxR = 1`, `CxRSEL = 0` y `FVREN = 1` |

`invert=True` pone `CxPOL`: la salida se invierte. La tabla 8-1: con `CxPOL = 0`, `CxOUT = 1` cuando V(+) > V(−). **Un comparador apagado** (`CxON = 0`) manda un 0 al XOR, así que su `out()` vale su polaridad (`True` si se inició con `invert=True` y se paró).

### Tiempos

Todas las esperas son **ciclos exactos** a partir del reloj declarado (como `ADC.read`):

| Espera | Valor | Dónde |
|---|---|---|
| Cambio de modo del comparador hasta salida válida | **10 µs** máximo (CM05) | al final de `CMPx.start` |
| Referencia fija de 0,6 V | **100 µs** máximo (VR02, solo caracterizado, no probado) | `CMPx.start(positive=REF_FIXED)` |
| Asentamiento de CVREF | **10 µs** máximo (CV04) | `CVREF.start` y `CVREF.set` |

El **tiempo de respuesta** al cambiar una entrada (CM04: 150 ns típico y 600 ns máximo al caer, 200 ns y 1 000 ns al subir) **no** lo espera nadie: un `out()` justo después de cambiar una tensión externa puede leer el nivel anterior. La interrupción del comparador (`C1IF` en `PIR2`) se maneja con `pyxpic.regs` (`PIR2.C1IF`, `PIE2.C1IE`); `start` la deja en 0 (leer `CMxCON0` borra la condición de discrepancia, 8.4, y luego baja la bandera).

### Condiciones eléctricas

El compilador no conoce el circuito. Del datasheet (tabla 17-7): offset de entrada **±10 mV** máximo (CM01), tensión de modo común **0 a VDD − 1,5 V** (CM02): las dos entradas deben estar en ese rango o el resultado no es fiable. Una entrada analógica no debe quedar flotando.

## CVREF

| Operación | Qué hace |
|---|---|
| `CVREF.start(step=n, low_range=False)` | `VRCON = VREN + rango + VR<3:0>` (`VRSS = 0`: la escalera va de VSS a VDD; `VROE = 0`: no sale al pin RA2) y espera 10 µs |
| `CVREF.set(n)` | cambia `VR<3:0>` sin tocar el rango y espera 10 µs |
| `CVREF.stop()` | `VRCON = 0` |

`step` es una constante de 0 a 15 (se escribe en el registro al compilar). La ecuación 8-1 con `VRSS = 0`:

| Rango | `low_range` | CVREF |
|---|---|---|
| alto (`VRR = 0`) | `False` | VDD/4 + (paso/32) × VDD: de 0,250 a 0,719 × VDD |
| bajo (`VRR = 1`) | `True` | (paso/24) × VDD: de 0 a 0,625 × VDD |

La referencia es ratiométrica (sigue a VDD, 8.10.4) y no llega a VSS ni a VDD por la construcción de la escalera. Precisión CV02: ±½ LSb. Con `CVREF.stop()` mientras un comparador sigue usando `REF_CVREF`, la escalera sigue alimentada (`C1RSEL` o `C2RSEL` la mantienen, figura 8-8): para apagarla de verdad apaga también los comparadores.

## Lo que NO hace

- **Sacar `CxOUT` o CVREF a un pin** (`CxOE`, `VROE`): hoy no hay una forma de pedirlo; con `pyxpic.regs` se escribe `CM1CON0.C1OE` (y `TRISA4 = 0`).
- **Sincronizar `C2OUT` con Timer1, usarlo de compuerta** (`C2SYNC`, `T1GSS`) ni **el pestillo SR** (`SRCON`): es el módulo `latch` de #45.
- **Despertar del sueño** o **apagar el PWM** con un comparador (la entrada de apagado del ECCP): no están.
- **Referencias externas** (`VREF+` y `VREF-`, `VRSS = 1`): solo VDD y VSS.
- **Un paso variable de CVREF**: `step` es una constante.
- **`start` y `set` dentro de una interrupción**: llevan una espera y las esperas no se permiten en una interrupción (como `pause`); `out()` sí.

## Verificación

El simulador del núcleo modela `CMxCON0` (`CxOUT` y `CxPOL`), la copia `MCxOUT` de `CM2CON1`, `CxIF` en cada cambio, CVREF por la ecuación 8-1 y la referencia fija con `FVREN`. Las entradas analógicas son las cuentas de 10 bits de `adc_inputs` (0 a 1 023 = VSS a VDD) y la referencia fija de 0,6 V se convierte con VDD = 5 V. Las pruebas diferenciales (`crates/pyxpic-pic14/tests/comparator.rs`) comparan el intérprete de IR con el simulador para las cuatro entradas −, las tres referencias, los 16 pasos de los dos rangos de CVREF y la polaridad. **Sin pruebas en Proteus ni en hardware.** No se modelan el tiempo de respuesta (CM04), el offset ni la condición de discrepancia con lectura de `CMxCON0` (la bandera sube en cada cambio de nivel).

## Reglas

| Código | Regla |
|---|---|
| `PYP2214` | `pyxpic.periph.comparator` no exporta ese nombre o el dispositivo no trae `COMPARATORS` |
| `PYP2215` | Argumentos de `CMPx.start`: solo con nombre, `negative` y `positive` obligatorios, constantes válidas, `invert` es `True` o `False`; argumentos con nombre en `stop` y `out` |
| `PYP2216` | Método que no existe (`start`, `stop`, `out`) |
| `PYP2217` | `CMPx` o una constante (`C12IN0`, `REF_PIN`...) usada como valor |
| `PYP2218` | `REF_CVREF` sin ninguna llamada a `CVREF.start()` en el programa (se comprueba que exista, no el orden) |
| `PYP2220` | `pyxpic.periph.vref` no exporta ese nombre (`CVREF`) o el dispositivo no trae `VREF` |
| `PYP2221` | Argumentos de `CVREF.start` y `CVREF.set`: `step` es una constante de 0 a 15, `low_range` es `True` o `False` |
| `PYP2222` | Método que no existe (`start`, `set`, `stop`) |
| `PYP2223` | `CVREF` usado como valor |
| `PYP2150` | un pin de entrada del comparador también se importa como pin de E/S (`RA0.output()`...) |
| `PYP2055` | Número de argumentos equivocado |
