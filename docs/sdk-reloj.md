# Reloj: fuente y frecuencia en una línea (`CLOCK: ...`)

**Estado: implementado (#69).** Una sola línea elige la fuente del reloj y su frecuencia, **sin imports**:

```python
CLOCK: Internal[4 * MHz]           # oscilador interno
CLOCK: Crystal[20 * MHz]           # cristal en OSC1/OSC2
CLOCK: ExternalClock[20 * MHz]     # señal de reloj en CLKIN (RA7)
```

`Internal`, `Crystal`, `ExternalClock`, `MHz`, `kHz` y `Hz` están predeclarados (como `uint8` o `Const`); `from pypic.clock import ...` es válido y no hace nada. La frecuencia es una **constante** (`20 * MHz`, `32768 * Hz`, `125 * kHz`). La forma anterior, `CLOCK_HZ: Const[uint32] = 4_000_000`, **sigue valiendo** para quien configure el oscilador a mano; no se pueden usar las dos a la vez (`PYP2192`).

Se eligió la sintaxis `Nombre[frecuencia]` porque es Python válido: `CLOCK_HZ: 4MHz internal` no lo es (`4MHz` no se lee como número).

## Qué hace el compilador con cada una

| Declaración | `FOSC` (palabra de configuración) | Otros efectos |
|---|---|---|
| `Internal[f]` | `INTRC_NOCLKOUT` (E/S en RA6 y RA7) | Escribe `OSCCON` (`IRCF`) y espera a que el oscilador esté estable, salvo con 4 MHz (el valor de reset) |
| `Crystal[f]` | `LP`, `XT` o `HS` según `f` (ver abajo) | Reserva RA7 (OSC1) y RA6 (OSC2) |
| `ExternalClock[f]` | `EC` (CLKIN en RA7, E/S en RA6) | Reserva RA7 |

También fija la frecuencia de `pause`, `delay_us`, la UART, el ADC y los temporizadores, igual que `CLOCK_HZ`, y `pypic build` imprime el resultado: `Oscilador: Crystal 20 MHz (FOSC = HS)` y `Configuracion: CONFIG1=0x20D2 ... (FOSC = HS por CLOCK)`.

### Límites (DS40001291H, tabla 17-1, p.249, y registro 4-1, p.64)

| Fuente | Valores válidos |
|---|---|
| `Internal` | **31 kHz** (LFINTOSC nominal), 125 kHz, 250 kHz, 500 kHz, 1, 2, **4 (reset)** y 8 MHz. Ninguna otra: `Internal[20 * MHz]` es el error `PYP2194` |
| `Crystal` | **LP**: 30 a 37 kHz (el cristal de 32,768 kHz) · **XT**: 100 kHz a 4 MHz · **HS**: más de 4 MHz hasta 20 MHz. Entre 37 y 100 kHz o por encima de 20 MHz no hay modo (`PYP2194`) |
| `ExternalClock` | De 1 Hz a 20 MHz (modo EC, de continua a 20 MHz) |

Entre 1 y 4 MHz el cristal podría usar XT o HS (la tabla permite ambos): se elige **XT**, de menor ganancia. Si necesitas HS ahí, declara `CLOCK_HZ` y elige `--config FOSC=HS`.

## `--config FOSC` y `CLOCK`

`CLOCK` **fija** `FOSC`: pedir otro valor con `--config FOSC=...` es un error (`el programa declara 'CLOCK: ...', que fija FOSC = HS`). Con `CLOCK_HZ` a secas el oscilador sigue siendo del usuario, y el aviso `PYP3005` ("el compilador todavía no configura el oscilador") se mantiene para ese caso, que es el que sigue siendo cierto.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP2190` | `CLOCK` mal declarado: no está a nivel de módulo, no se llama `CLOCK`, lleva `= valor` o `pypic.clock` no exporta ese nombre |
| `PYP2192` | `CLOCK` y `CLOCK_HZ` a la vez, o `CLOCK` dos veces |
| `PYP2193` | La frecuencia no es una constante mayor que 0 |
| `PYP2194` | La fuente no admite esa frecuencia (con los valores válidos en el mensaje) |
| `PYP2150` | RA6 o RA7 se importan como E/S y el oscilador externo los usa |

## Lo que no hace, y lo que no puede comprobar

- **El circuito:** que el cristal esté puesto, sus capacitores o que el reloj externo llegue. El compilador solo valida lo que dice el datasheet.
- **Cambiar de reloj en marcha, Two-Speed Start-up, el monitor de falla (Fail-Safe Clock Monitor), `PLL` y los modos `RC`/`RCIO`:** no se tocan (`RC` se elige a mano con `--config FOSC=EXTRC_NOCLKOUT` y `CLOCK_HZ`).
- **Desviación del oscilador interno:** `Internal[4 * MHz]` es nominal; el PIC16F887 tiene tolerancia (se calibra con `OSCTUNE`, que no se toca). Para una UART exacta conviene un cristal.

## Verificación

`crates/pypic-pic14/tests/clock.rs`: una línea sin imports; el modo `FOSC` elegido para cada frecuencia (incluidos los bordes 100 kHz, 4 MHz, 4 000 001 Hz y 20 MHz); las ocho frecuencias del oscilador interno con su `IRCF`; los rechazos con los límites en el mensaje; los errores de declaración; la reserva de pines (`RA6`, `RA7` con cristal, `RA7` con EC, nada con el interno); el número exacto de ciclos de una `pause` con cada frecuencia (4, 8, 20, 1 y 2 MHz); que el oscilador interno a 4 MHz **no escribe** `OSCCON` y los demás sí, esperando la bandera de estabilidad; y que el simulador sigue a `OSCCON` para que la UART use el reloj real. En `crates/pypic-cli/tests/cli.rs`: `FOSC = HS` en la palabra de configuración y el rechazo de `--config FOSC`. Con `Internal[4 * MHz]` el `.hex` de `examples/uart/adc_serial.py` sale **idéntico** al que ya funcionó en Proteus. Ejemplo: `examples/reloj_cristal.py`. **No probado en hardware:** el simulador modela `OSCCON` y su estabilidad de forma mínima (12 ciclos), no los tiempos reales del oscilador.
