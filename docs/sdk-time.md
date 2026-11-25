# SDK: manejo del tiempo (`pypic.system.time`)

**Estado: esperas activas implementadas; `nap`/`sleep` definidas y reservadas.** Todo se resuelve al compilar a instrucciones nativas
(sin runtime). La razón de ser de un PIC es su **manejo del tiempo preciso y determinista**, y por eso el compilador trabaja en la unidad del hardware: el **ciclo de instrucción**.

```python
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]

@init_device
def configurar() -> None:
    RD0.output()

def main() -> None:
    while True:
        RD0.toggle()
        pause(500)          # 500 ms exactos
```

## Una sola forma por operación

PyPIC no tiene sinónimos: cada operación tiene **un** nombre, para evitar ambigüedades. Los nombres siguen a PICAXE cuando existe equivalente.

| Función | Unidad | Argumento | Mecanismo | Precisión |
|---|---|---|---|---|
| `delay_cycles(n)` | ciclos de instrucción | constante | espera activa | **exacta**: la unidad mínima del hardware |
| `delay_us(n)` | microsegundos | constante | espera activa | exacta al ciclo más cercano (aviso si no cae en ciclos enteros) |
| `pause(n)` | milisegundos (como `PAUSE` de PICAXE) | constante o `uint16` | espera activa | exacta con constante; con variable, `n` ms + 8 ciclos fijos |
| `wait(n)` | segundos | constante o `uint16` | espera activa | igual que `pause` |
| `nap(n)` | *(por definir)* | | bajo consumo (watchdog) | aproximada. **Reservada**, distinta de `sleep` |
| `sleep(n)` | *(por definir)* | | bajo consumo (watchdog) | aproximada. **Reservada**, distinta de `nap` |

`delay_ms` y `delay` **no existen** (duplicarían `pause`): importarlas da un error que dice cuál usar. `nap` y `sleep` dan hoy el error `PYP2103`; se implementarán
como dos funciones con el comportamiento de PICAXE tras leer la sección del watchdog del datasheet.

## Cómo se garantiza la precisión

- **Constante → ciclos exactos.** El compilador convierte el tiempo en un número entero de ciclos (`ciclos = tiempo × frecuencia del reloj / (relojes por ciclo × unidad)`, redondeado una sola vez
  sobre el total, sin acumular error por milisegundo) y genera código que dura **exactamente** ese número, con 1, 2 o 3 contadores y relleno. Verificado contra el simulador
  para cualquier cantidad de 0 a 400, en todos los umbrales, hasta 100 millones de ciclos y con valores pseudoaleatorios.
- **Variable (`uint16`)**: `n` vueltas de exactamente 1 unidad más **8 ciclos fijos** de entrada (9 si `n = 0`).
- **Un ciclo de instrucción = 4 relojes** en el PIC16F887 (DS40001291H p.1: 20 MHz → 200 ns); el dato sale del Device Pack (`cpu.clocks_per_instruction`).
- **Informe de tiempos**: `pypic build prog.py --timing` muestra, por cada espera, lo pedido, los ciclos emitidos, el tiempo real y el error.
- **Avisos**: `PYP3003` si el tiempo pedido no cae en ciclos enteros (con el error de redondeo), `PYP3002` si con cantidad variable una unidad no es un número entero de ciclos.
- Las esperas son **activas**: la CPU sigue ejecutando, por lo que **una interrupción alarga la espera** en lo que dure su rutina. Por eso no pueden usarse dentro de la interrupción (`PYP2102`).

## Reloj

- `CLOCK: Internal[4 * MHz]` (o `Crystal[...]` / `ExternalClock[...]`, ver `docs/sdk-reloj.md`) declara la fuente y la frecuencia del reloj del sistema y **configura el oscilador**. La forma manual `CLOCK_HZ: Const[uint32] = <hercios>` solo declara la frecuencia.
- **Sin reloj declarado** el compilador usa el reloj por defecto del dispositivo (PIC16F887: 4 MHz, `OSCCON<IRCF> = 110`, p.64) y lo **avisa** (`PYP3004`, que sugiere `CLOCK: Internal[4 * MHz]`).
- Con la forma manual `CLOCK_HZ`, si declaras un reloj distinto del que da la configuración por defecto, avisa (`PYP3005`): el compilador **no programa el oscilador** (con `CLOCK: ...` sí lo hace) (`OSCCON`, palabras de configuración, #33), así que los
  tiempos solo serán correctos si el reloj real es el declarado. En Proteus, la propiedad *Processor Clock Frequency* del PIC debe coincidir.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP2100` | `delay_us`/`delay_cycles` necesitan una constante |
| `PYP2101` | Falta el reloj y el dispositivo no tiene uno por defecto |
| `PYP2102` | Espera dentro de la interrupción (o de una función que ella llama) |
| `PYP2103` | `nap`/`sleep` todavía no implementadas |
| `PYP2104` | `delay_ms`/`delay` no existen: una sola forma por operación |
| `PYP3002`–`PYP3005` | Avisos de precisión y de reloj |

## Memoria

Cada espera usa 5 bytes de contadores (compartidos por todas las esperas del programa) en el banco 0, solo si el programa las usa.

## Timers

Los temporizadores son periféricos y tienen su propio documento: `docs/sdk-timer.md` (`from pypic.periph.timer import TMR2`).
