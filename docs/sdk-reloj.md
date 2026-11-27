# Reloj: `CLOCK` y `FREQUENCY` (y literales con unidad)

**Estado: implementado (#69), fases 1 a 5.** El reloj se declara en dos líneas, **sin imports**: la fuente y su frecuencia por separado.

```python
CLOCK: Internal                 # oscilador interno; sin FREQUENCY es 4 MHz (el valor de reset)

CLOCK: Internal
FREQUENCY: 8MHz                 # oscilador interno con frecuencia seleccionable

CLOCK: Crystal                  # cristal en OSC1/OSC2
FREQUENCY: 11.0592MHz           # cualquier valor dentro del rango del modo
```

`MHz`, `kHz` y `Hz` siguen predeclarados (`8 * MHz` vale), `from pyxpic.clock import ...` es válido y no hace nada, y `CLOCK_HZ: Const[uint32] = 4_000_000` sigue siendo la **forma manual**: solo declara la frecuencia y **no configura el oscilador**. No se pueden usar las dos formas a la vez (`PYP2192`).

La declaración **reemplaza** a las formas anteriores `Clock: Internal(8MHz)` y `CLOCK: Internal[4 * MHz]` (error `PYP2190` con el remedio) y a `ExternalClock` (ahora es `CLOCK: External`).

## Fuentes (`CLOCK`)

| Fuente | Qué es | `FREQUENCY` |
|---|---|---|
| `Internal` | Oscilador interno | Opcional (4 MHz); solo las frecuencias que el dispositivo configura por registro |
| `Crystal` | Cristal de cuarzo en OSC1/OSC2 | Obligatoria; cualquier valor dentro del rango de un modo |
| `External` | Señal de reloj generada por otro circuito (CLKIN) | Obligatoria |
| `Resonator` | Resonador cerámico en OSC1/OSC2 | Obligatoria; mismos modos que el cristal |
| `RC` | Resistencia y capacitor externos | Obligatoria (la frecuencia depende del circuito) |
| `Secondary` | Oscilador secundario (Timer1) | Solo en dispositivos que lo admiten como reloj del sistema |

`OSC_MODE: HS` (opcional) elige el modo del oscilador externo; sin él, `Auto` toma el primer modo del dispositivo cuyo rango contiene la frecuencia. `OSC_MODE` con `Internal` es un error: el modo del interno lo fija el dispositivo.

## Los límites salen del dispositivo

Cada fuente y cada modo se validan contra el perfil **del dispositivo**, no contra una tabla global de la familia PIC: el Device Pack trae `oscillator.json` con las frecuencias del oscilador interno (y su `IRCF`), los modos externos con su rango y los pines que ocupan. Cambiar de dispositivo cambia lo que vale: el oscilador interno llega a 8 MHz en el PIC16F887, a 16 MHz en los PIC18 K22 y a 64 MHz en los K42/Q43. Sin Device Pack (pruebas del frontend con un perfil suelto) se usa la tabla del PIC16F887.

| PIC16F887 (`devices/pic16f887/oscillator.json`) | Valores |
|---|---|
| `Internal` | **31 kHz** (LFINTOSC nominal), 125 kHz, 250 kHz, 500 kHz, 1, 2, **4 (reset)** y 8 MHz (DS40001291H, registro 4-1, p.64). Otra: `PYP2194` |
| `Crystal` / `Resonator` | **LP** 30–37 kHz (el cristal de 32,768 kHz) · **XT** 100 kHz–4 MHz · **HS** 1–20 MHz (tabla 17-1, p.249) |
| `External` | **EC**, de 1 Hz a 20 MHz (tabla 17-1) |
| `RC` | **EXTRC**, sin límite propio en la tabla 17-1: el del dispositivo (20 MHz) |
| `Secondary` | no admitido como reloj del sistema en este dispositivo |

Con `Auto`, entre 1 y 4 MHz se elige **XT** (menor ganancia); `OSC_MODE: HS` fuerza HS ahí. Los pines que ocupa cada fuente: `Crystal`/`Resonator` toman RA7 (OSC1) y RA6 (OSC2); `External` solo RA7 (CLKIN); `RC` RA7; `Internal`, ninguno. El compilador **avisa** (`PYP3016`) y, importarlos como E/S, es el error `PYP2150`.

## Qué hace el compilador

| Declaración | `FOSC` (palabra de configuración) | Otros efectos |
|---|---|---|
| `Internal` | `INTRC_NOCLKOUT` (E/S en RA6 y RA7) | Escribe `OSCCON` (`IRCF`) y espera a que el oscilador esté estable, salvo con 4 MHz (el valor de reset) |
| `Crystal`, `Resonator` | `LP`, `XT` o `HS` | Reserva RA7 y RA6 |
| `External` | `EC` | Reserva RA7 |
| `RC` | `EXTRC_NOCLKOUT` | Reserva RA7 |

También fija la frecuencia de `pause`, `delay_us`, la UART, el ADC y los temporizadores, y `pyxpic build` imprime `Oscilador: Crystal 11.0592 MHz (FOSC = HS)`. `CLOCK` **fija** `FOSC`: pedir otro valor con `--config FOSC=...` es un error.

### Constantes del reloj

El compilador define, a partir del reloj efectivo (declarado, `CLOCK_HZ` o el del dispositivo): `SOURCE_FREQ` (frecuencia de la fuente), `FOSC` (frecuencia del sistema; sin PLL ni divisores es igual a `SOURCE_FREQ`) y `FCY` (ciclos de instrucción por segundo, `FOSC / 4` en PIC16). Son constantes de compilación (sin RAM ni código): `RELOJ: Const[uint32] = FCY`. Se resuelven con lo declarado hasta el punto de uso, así que `CLOCK` y `FREQUENCY` van al principio.

## Literales con unidad

Un número decimal pegado a un sufijo es un **literal con unidad**: el lexer lo convierte a un entero exacto en la unidad base de su magnitud, con aritmética de enteros de 128 bits (sin punto flotante, sin RAM, sin código). Si el valor no es un número entero de unidades base (`0.5Hz`) es el error `PYP0009`. Admite `_`, parte decimal y exponente: `8MHz`, `11.0592MHz`, `32.768kHz`, `11_059_200Hz`, `8e6Hz`, `500ms`, `3.3V`, `50pct`. Los sufijos distinguen mayúsculas (`MHz` frecuencia, `mH` inductancia, `MB` memoria, `mV` voltaje).

| Magnitud | Sufijos | Unidad base |
|---|---|---|
| Frecuencia | `Hz` `kHz` `MHz` `GHz` | Hz |
| Tiempo | `ns` `us` `ms` `s` `min` `h` `H` | ns |
| Memoria | `B` `KB` `MB` `KiB` `MiB` | B |
| Comunicación serial | `baud` `kbaud` `Mbaud` | baud |
| Transferencia de datos | `bps` `kbps` `Mbps` | bps |
| Voltaje | `uV` `mV` `V` | uV |
| Corriente | `uA` `mA` `A` | uA |
| Resistencia | `ohm` `kohm` `Mohm` | mohm |
| Capacitancia | `pF` `nF` `uF` `mF` `F` | pF |
| Inductancia | `nH` `uH` `mH` `henry` | nH |
| Potencia | `uW` `mW` `W` | uW |
| Temperatura | `degC` `degF` `K` | mK (milikelvin) |
| Rotación | `rpm` | rpm |
| Ángulo | `deg` `rad` | udeg (`rad` se redondea al microgrado) |
| Porcentaje | `pct` | mpct (milésimas de punto porcentual) |

Se pueden combinar con expresiones constantes: `4MHz * 2`, `16MHz / 2`, `8 * MHz`, `2MHz + 2MHz`. El verificador conoce la magnitud de cada expresión: **sumar o restar magnitudes distintas** (`4MHz + 1ms`) o un valor con un número sin unidad es `PYP2231`; multiplicar dos magnitudes (`MHz * MHz`) o dividir un número entre una magnitud también; dividir dos de la misma da un número. `FREQUENCY` exige una frecuencia (`PYP2232`: `FREQUENCY: 4` o `FREQUENCY: 3ms`).

Los valores de las magnitudes cuya unidad base no era un entero del lenguaje (tiempo, voltaje, corriente...) **no se aceptan donde se espera un número** (`X: uint32 = 500ms` es `PYP2230`): solo los consume un parámetro que declara esa magnitud. Las unidades no dependen del microcontrolador; que un dispositivo pueda generar o medir un valor lo valida cada función del SDK con su Device Pack.

## Ajustes complementarios

Opcionales, a continuación de `CLOCK` (y `FREQUENCY`); cada uno se valida contra el perfil del dispositivo. Pedir lo que el dispositivo no tiene es un error (`PYP2194`) que dice qué sí tiene; apagarlo (`Off`, `PLL: Off`, `CLOCK_DIVIDER: 1`) siempre es válido.

| Declaración | Qué hace | PIC16F887 |
|---|---|---|
| `PLL: 4` / `PLL: On` / `PLL: Off` | Multiplica la fuente: `FOSC = SOURCE_FREQ * PLL / CLOCK_DIVIDER`. `On` solo basta si el dispositivo tiene un único multiplicador. Valida la fuente y el rango de entrada de la PLL | no tiene PLL |
| `CLOCK_DIVIDER: 2` | Divide la frecuencia; debe ser exacta | no tiene divisores |
| `CLOCK_OUTPUT: On` | Saca el reloj por el pin CLKOUT (`FOSC` con `CLKOUT`): ocupa el pin | `Internal` y `RC` → RA6 (`INTRC_CLKOUT`, `EXTRC_CLKOUT`); LP/XT/HS usan OSC2 y EC no tiene salida |
| `FAIL_SAFE_CLOCK: On` | Monitor de fallas del oscilador (bit `FCMEN` de CONFIG1) | modos externos LP, XT, HS, EC y RC (DS40001291H 4.8) |
| `CLOCK_SWITCHING: On` | Cambio interno/externo al arrancar (Two-Speed Start-up, bit `IESO`) | LP, XT y HS (DS40001291H 4.7) |

La frecuencia resultante (`FOSC`) se valida contra el máximo del dispositivo (`PYP2094`) y es la que usan `pause`, la UART, el ADC y los temporizadores; `SOURCE_FREQ` sigue siendo la de la fuente. Los bits de configuración que piden estos ajustes los pone el compilador: pedir otro valor con `--config FCMEN=...` es una contradicción. No cambia de reloj en marcha (`OSCCON<SCS>`): eso sigue siendo manual.

### Qué falta (fases siguientes)

- La PLL y el divisor se probaron con un perfil sintético: ningún dispositivo del repositorio las tiene todavía; su `oscillator.json` (`pll`, `dividers`) se llenará con el primer PIC que las traiga.
- **Fase 4 (hecha):** unidades en los parámetros del SDK, ver `docs/unidades.md`.
- **Fase 5 (hecha):** BASIC con literales con unidad y los ajustes del reloj, y resaltado de los sufijos en VS Code (`docs/unidades.md`).

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP0008` / `PYP0009` | Literal con unidad demasiado grande / no es un número entero de unidades base |
| `PYP2190` | `CLOCK`, `FREQUENCY` u `OSC_MODE` mal declarados: fuera del nivel de módulo, otro nombre, la forma antigua `Clock: ...(...)`, `ExternalClock`, `FREQUENCY` sin `CLOCK`, lleva `= valor` |
| `PYP2192` | `CLOCK` y `CLOCK_HZ` a la vez, o cualquiera de las tres declaraciones dos veces |
| `PYP2193` | Falta `FREQUENCY` (`Crystal`, `External`...), o no es una constante mayor que 0 |
| `PYP2194` | La fuente o el modo no admiten esa frecuencia, o el dispositivo no tiene esa fuente (con los valores válidos en el mensaje) |
| `PYP2094` | La frecuencia (o el `FOSC` resultante con PLL y divisor) excede el reloj máximo del dispositivo |
| `PYP2150` | RA6 o RA7 se importan como E/S y el oscilador externo los usa |
| `PYP2230` | Una magnitud con unidad (tiempo, voltaje...) donde ningún parámetro la acepta |
| `PYP2231` | Operación entre magnitudes incompatibles |
| `PYP2232` | `FREQUENCY` no es una frecuencia o no lleva unidad |
| `PYP3016` | Aviso al declarar un oscilador externo: pines que dejan de estar disponibles |

## Lo que no hace, y lo que no puede comprobar

- **El circuito:** que el cristal esté puesto, sus capacitores o que el reloj externo llegue. El compilador solo valida lo que dice el datasheet.
- **Cambiar de reloj en marcha (`OSCCON<SCS>`):** no se toca. El modo `RC` no valida la frecuencia contra el circuito.
- **Desviación del oscilador interno:** `Internal` a 4 MHz es nominal; el PIC16F887 tiene tolerancia (se calibra con `OSCTUNE`, que no se toca). Para una UART exacta conviene un cristal.

## Verificación

`crates/pyxpic-frontend/src/units.rs` (pruebas unitarias del sistema de sufijos: exactitud, mayúsculas, números mal formados) y `crates/pyxpic-pic14/tests/clock.rs`: el modo `FOSC` elegido para cada fuente y frecuencia (incluidos los bordes 100 kHz, 4 MHz, 4 000 001 Hz, 20 MHz y 11,0592 MHz); las ocho frecuencias del oscilador interno con su `IRCF`; los rechazos con los límites en el mensaje; `OSC_MODE`; los errores de declaración y de magnitudes; la reserva de pines; que los límites cambian con el perfil del dispositivo; las constantes `SOURCE_FREQ`/`FOSC`/`FCY`; que BASIC y Python declaran el mismo reloj; el número exacto de ciclos de una `pause` a 4, 8, 20, 1 y 11,0592 MHz; que `Internal` a 4 MHz **no escribe** `OSCCON` y los demás sí; y que el simulador sigue a `OSCCON` para que la UART use el reloj real. **No probado en hardware:** el simulador modela `OSCCON` y su estabilidad de forma mínima (12 ciclos), no los tiempos reales del oscilador.
