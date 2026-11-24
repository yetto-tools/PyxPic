# SDK: puerto serie (`pypic.periph.uart`)

**Estado: implementado (segundo módulo de #45).** EUSART del PIC16F887 en modo **asíncrono 8N1** (8 bits de datos, sin paridad, un bit de parada), transmisión y recepción (DS40001291H, cap. 12). Todo se resuelve al compilar.

```python
from pypic.periph.uart import UART

CLOCK_HZ: Const[uint32] = 4_000_000

UART.start(baud=9600)        # SPBRG, BRGH, BRG16 calculados; SPEN, TXEN y CREN encendidos
UART.write(65)               # espera TXIF y escribe TXREG
dato: uint8 = UART.read()       # espera RCIF y lee RCREG
```

Los pines son **RC6/TX** y **RC7/RX**. Con `SPEN` el chip los toma solos (TX pasa a salida y RX a entrada, sin tocar TRIS: p.153), así que importarlos como E/S (`RC6`, `RC7`) es el error `PYP2150`.
Un nivel RS-232 (±12 V) o RS-485 necesita **un transceptor externo** (MAX232, MAX485...): el PIC solo da niveles lógicos.

## Operaciones

| Operación | Qué hace | Ciclos |
|---|---|---|
| `UART.start(baud=N)` | Configura y enciende: `BAUDCTL`, `SPBRGH:SPBRG`, `TXSTA` (TXEN), `RCSTA` (SPEN, CREN) | 20 |
| `UART.write(x)` | Espera `TXIF` y envía el `uint8` | 5 con el transmisor libre |
| `UART.read()` → `uint8` | Espera `RCIF` y lee | 4 con un byte esperando |
| `UART.send(x)` | Escribe `TXREG` **sin esperar**, y un `NOP` | 3 |
| `UART.recv()` → `uint8` | Lee `RCREG` sin esperar | 2 |
| `UART.tx_ready()` → `bool` | `TXIF`: `TXREG` está vacío | 3 |
| `UART.rx_ready()` → `bool` | `RCIF`: hay un carácter sin leer | 3 |
| `UART.sending()` → `bool` | El registro de desplazamiento sigue ocupado (`TRMT` = 0): sirve para esperar a que **salga el último bit** | 7 |
| `UART.overrun()` → `bool` | `OERR`: llegó un tercer carácter antes de leer el FIFO | 3 |
| `UART.framing_error()` → `bool` | `FERR` del carácter que está a punto de leerse: **se lee antes** que `read()`/`recv()` | 3 |
| `UART.clear_overrun()` | `CREN` a 0 y a 1: la única forma de borrar `OERR` (12.1.2.5) | 2 |

`write` y `read` **son exactamente** `tx_ready`/`rx_ready` + `send`/`recv`, no sinónimos: con las primitivas el programa no se bloquea esperando un byte, y puede hacer otra cosa mientras el chip transmite.
Todas valen **dentro de una interrupción** (no usan memoria compartida ni esperas fijas); `write` y `read` además se quedan sondeando, así que en una ISR se prefieren `rx_ready()` + `recv()`.

## El `NOP` de `send`

`TXIF` solo es válido **en el segundo ciclo de instrucción** tras escribir `TXREG` (p.150): una instrucción que lo lea justo después ve el valor viejo (1) aunque el byte ya esté en cola, y un segundo `send` pisaría al primero. Por eso `send` termina con un `NOP` y
`tx_ready()` inmediatamente después dice la verdad. El simulador modela esa ventana y avisa si un byte se pierde por escribir sobre `TXREG` ocupado.

## El generador de baudios

El compilador prueba los **cuatro modos** de la tabla 12-3 (p.160) y elige el de **menor error**, con el `n` entero más cercano por arriba y por abajo; a igual error, el más simple (8 bits, `BRGH` = 0):

| `BRG16` | `BRGH` | Baudios |
|---|---|---|
| 0 | 0 | `FOSC / [64 (n+1)]` |
| 0 | 1 | `FOSC / [16 (n+1)]` |
| 1 | 0 | `FOSC / [16 (n+1)]` |
| 1 | 1 | `FOSC / [4 (n+1)]` |

`pypic build --timing` imprime la velocidad real y su error: `uart: 9600 baudios pedidos -> 9615.4 (+0.16 %), generador de 8 bits, BRGH = 1, SPBRG = 25 a 4000000 Hz`.

**La tolerancia es una decisión de PyPIC, no del datasheet** (que no da un límite): una trama 8N1 son 10 bits y el receptor muestrea cada bit hacia su centro; con un error total mayor de ~4,7 % entre los dos extremos el último bit se lee fuera de su ventana.
Se **avisa por encima de 2,0 %** (`PYP3014`: deja margen para que el otro extremo tenga su error) y es un **error por encima de 4,5 %** (`PYP2182`: aunque el otro extremo fuera exacto, fallaría). Por ejemplo, a 4 MHz y 115200 baudios sale −3,55 % (aviso) y 230400 da +8,51 % (error).

## Reglas

| Código | Regla |
|---|---|
| `PYP2180` | `pypic.periph.uart` no exporta ese nombre (solo `UART`), o el dispositivo no tiene EUSART |
| `PYP2181` | Se usa la UART pero no hay un `UART.start(baud=...)` que `main` o `@init_device` puedan ejecutar |
| `PYP2182` | `start` mal formado (hace falta `baud=`, una constante de 50 a 3 000 000) o el error de baudios pasa de 4,5 % |
| `PYP2183` | Método que no existe |
| `PYP2185` | `UART` usado como valor |
| `PYP2186` | Dos llamadas a `UART.start` con velocidades distintas (el EUSART tiene un solo generador) |
| `PYP2150` | `RC6` o `RC7` también se importan como E/S |
| `PYP3014` | Error de baudios mayor de 2,0 % |

Igual que el ADC, el compilador comprueba que el `start()` **exista y sea alcanzable**, **no el orden**: enviar antes de iniciar compila y el simulador lo registra (`TXREG escrito con el transmisor apagado`).

## Lo que no está (todavía)

- **9 bits de datos**, paridad (hay que calcularla a mano con 9 bits), **modo síncrono**, detección automática de baudios, `SENDB` y LIN: issues aparte.
- **Interrupciones**: `RCIF`/`TXIF` se usan con `pypic.regs` (`PIE1.RCIE.set()`, `INTCON.PEIE.set()`...) y se leen con `UART.rx_ready()` + `UART.recv()` en la ISR. No hay helpers `UART.rx_interrupt_on()` porque la ISR no puede compartir memoria con `main` sin un diseño propio.
- Un carácter que llega con la recepción apagada, o con `OERR` sin borrar, **se pierde** (como en el chip).

## Verificación

El simulador del núcleo modela el EUSART: `TXREG` y el registro de desplazamiento con la trama de 10 bits que sale de `SPBRG`/`BRGH`/`BRG16` (tabla 12-3), `TXIF` con su ventana de validez, `TRMT`, el FIFO de recepción de **dos caracteres**, `OERR` (tercer carácter), `FERR` (por carácter) y el borrado de `OERR` con `CREN`.
`crates/pypic-pic14/tests/uart.rs`: la **velocidad medida en el simulador** (a partir de los ciclos entre dos bytes) coincide con la pedida y con el error informado a nueve combinaciones de reloj y baudios; los cuatro modos; el eco contra el intérprete de referencia; FIFO, desbordamiento y error de trama; un byte perdido por
enviar sin esperar; el valor viejo de `TXIF` sin el `NOP`; los costos de arriba; y las reglas del lenguaje. Ejemplo: `examples/uart_echo.py`.
**No se ha probado con un puerto serie real.** El simulador usa la misma tabla 12-3 que el compilador: un reloj interno cuya frecuencia real se desvía de la nominal (el oscilador interno del PIC16F887 tiene tolerancia) desplaza los baudios y no se ve ahí.
