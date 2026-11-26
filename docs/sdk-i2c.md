# SDK: I²C maestro por hardware (`pypic.periph.i2c`)

**Estado: diseño (primera entrega de #45 para I²C, sin implementar).** El MSSP del PIC16F887 (DS40001291H, cap. 13, pp. 175-205). Esta página fija qué se construye y qué se leyó del datasheet; lo que falta por leer está marcado.

## Alcance de la primera entrega

Solo el **maestro de 7 bits** a 100 kHz y 400 kHz, en los pines fijos del MSSP (SCL = RC3, SDA = RC4). Sin esclavo, sin 10 bits, sin multi-maestro ni arbitraje (`BCLIF`), sin alargamiento de reloj del esclavo más allá de lo que hace el hardware. El I²C por software sobre cualquier pin es `pypic.soft.i2c` (#47).

```python
from pypic.device import PIC16F887
from pypic.periph.i2c import I2C

CLOCK: Crystal[8 * MHz]

I2C.start(freq=100_000)           # SSPADD = Fosc/(4*freq) - 1
if I2C.probe(0x27):               # START, dirección + W, ¿ACK?, STOP
    ...
```

| Operación | Qué hace |
|---|---|
| `I2C.start(freq=)` | Pone SCL/SDA como entradas (el MSSP los controla), `SSPCON = 0x28` (`SSPEN` y modo maestro, `SSPM = 1000`), `SSPADD` según el reloj, `SMP` según la velocidad. Solo con nombre |
| `I2C.probe(addr)` → `bool` | START, envía `addr << 1` (escritura), lee `ACKSTAT`, STOP. Devuelve `True` si el esclavo respondió ACK. No modifica el dispositivo |
| `I2C.stop()` | `SSPEN = 0`; los pines vuelven a ser E/S |

`probe` es una transacción de detección segura: solo direcciona (escritura de la dirección, sin datos). `write(addr, ...)`, `read(addr, n)` y el acceso a registros vienen en la segunda entrega.

## Lo leído del datasheet

| Dato | Valor | Página |
|---|---|---|
| `SSPSTAT.SMP` en maestro | 1 = control de pendiente desactivado (100 kHz y 1 MHz); 0 = activado (400 kHz) | 176 |
| `SSPSTAT.R/W` en maestro | 1 = transmisión en curso; **se hace OR con `SEN`, `RSEN`, `PEN`, `RCEN` y `ACKEN` para saber si el MSSP está en reposo** | 176 |
| `SSPCON.SSPEN` | habilita el puerto y entrega SDA/SCL al MSSP | 177 |
| `SSPCON.SSPM<3:0>` | `1000` = I²C maestro, reloj = Fosc / (4 × (SSPADD + 1)); `1011` = maestro controlado por firmware | 177 |
| `SSPCON.WCOL` | escribir `SSPBUF` sin estar el MSSP en reposo lo activa (hay que limpiarlo por software) | 177 |
| `SSPCON2.SEN / RSEN / PEN / RCEN / ACKEN` | inician START, START repetido, STOP, recepción y secuencia ACK; el hardware los baja solo | 178 |
| `SSPCON2.ACKSTAT` | 0 = el esclavo respondió ACK; 1 = no hubo ACK | 178 |
| Nota 1 de `SSPCON2` | si el módulo no está en reposo, esos bits no se pueden activar y escribir `SSPBUF` se ignora | 178 |

## Pendiente de leer antes de implementar

- Fórmula y límites del generador de baudios (13.4.5, p. 191) y el retardo mínimo entre eventos.
- Secuencia de transmisión maestro, `SSPIF` y los tiempos de START/STOP (pp. 189-198, figuras 13-15 y 13-16).
- Requisitos de los pull-ups externos en SDA y SCL (el PIC no los tiene; el compilador no puede verificarlos).
- Los tiempos de bus a 100 y 400 kHz (cap. 17, tablas de I²C) para documentar la velocidad real según el reloj.

## Cómo se prueba

Pruebas diferenciales como las de los otros módulos: intérprete de IR contra el simulador del núcleo, con un **esclavo virtual** en el simulador (una dirección que responde ACK) que modela START, dirección, `ACKSTAT` y STOP. Sin pruebas en Proteus ni en hardware hasta que el usuario las haga.

## Después

El escáner I²C (#77) es un programa de PyPIC que llama a `probe` de `0x08` a `0x77` y manda las respuestas por UART; la Terminal serial (#72) lo muestra.
