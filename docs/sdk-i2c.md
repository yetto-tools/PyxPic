# I²C maestro por hardware y pantalla LCD 16x2 (MSSP)

**Estado: `pyxpic.periph.i2c` existe con `start`, `begin`, `restart`, `end`, `write` y `read` (maestro de 7 bits). Está el modelo del MSSP en el simulador y los ejemplos `lcd_i2c.py` (con `pyxpic.regs`) y `escaner.py` (con el módulo).** El MSSP del PIC16F887 (DS40001291H, cap. 13, pp. 175-205) en modo maestro de 7 bits.

**Verificación.** Simulador de PyxPic: pruebas diferenciales. **Proteus (simulación, no hardware real):** el usuario vio `lcd_i2c.hex` mostrar «Hola PyPIC» (el nombre anterior del proyecto) e «I2C 100 kHz» en un `LM016L` (LCD de 16x2) con un `PCF8574` como pieza separada, con A0, A1 y A2 a VDD (dirección `0x27`) y RS, RW y E en P0 a P2, D4 a D7 en P4 a P7 (captura del 2026-11-26). El componente `JHD-2X16-I2C` de Proteus (con `Device Address = $7C`, `Row 1 = 80-8F`, `Row 2 = C0-CF`, `Clock Frequency = 100kHz`) **no es una mochila `PCF8574`**: tiene controlador I²C propio en `0x3E` (se escribe `0x7C`) y recibe un byte de control (`0x00` orden, `0x40` carácter) y el dato. `examples/python/i2c/lcd_controlador.py` le escribe las dos líneas y el usuario vio la información en la pantalla (captura del 2026-11-26). Con `lcd_i2c` (disposición de `PCF8574`) esa pantalla no mostraba texto aunque el bus y los bytes eran correctos (ACK en cada transacción). Con A0, A1 y A2 a tierra el `PCF8574` es `0x20` (se escribe `0x40`), no `0x27`. **Hardware real: sin probar.**

## El módulo `pyxpic.periph.i2c`

```python
from pyxpic.device import PIC16F887
from pyxpic.periph.i2c import I2C

CLOCK: Internal
FREQUENCY: 4MHz

def main() -> None:
    I2C.start(freq=100000)          # SSPADD, SSPSTAT, SSPCON; SCL (RC3) y SDA (RC4) como entradas
    I2C.begin()                     # START
    if I2C.write(0x4E):             # direccion de 7 bits (0x27) << 1 con R/W = 0; True si el esclavo respondio ACK
        I2C.write(0x08)             # un dato
    I2C.end()                       # STOP
```

| Método | Qué hace |
|---|---|
| `I2C.start(freq=N)` | `N` es una constante de 1000 a 1 000 000 Hz. Calcula `SSPADD = ceil(FOSC / (4 * N)) - 1` al compilar: **nunca más rápido** que lo pedido (p. 177, fórmula del reloj). Acepta `SSPADD` de 1 a 127 (decisión de PyxPic) y avisa (`PYP3019`) si la frecuencia real baja del 80 % de la pedida. Deja `SMP = 0` (control de pendiente activado) solo entre 100 y 400 kHz, `SMP = 1` en el resto; `SSPEN = 1` y `SSPM = 1000` al final |
| `I2C.begin()`, `I2C.restart()`, `I2C.end()` | `SEN`, `RSEN` y `PEN` = 1 y espera a que el hardware los baje |
| `I2C.write(b)` | escribe `SSPBUF`, espera a que `R/W` baje y devuelve `True` si `ACKSTAT` = 0. El resultado se puede ignorar: el byte se envía igual |
| `I2C.read(ack=x)` | `RCEN` = 1, espera, lee `SSPBUF` y responde con `ACKDT` y `ACKEN` (13.4.9 y 13.4.10): ACK si `x` es `True` (quiero otro byte), NACK si es `False` (el último, antes de `end` o `restart`). `ack` es obligatorio y puede ser una variable. Devuelve el byte |

Cada operación espera a que el MSSP termine, así que no hay eventos solapados ni `WCOL`. Un solo `start` por programa y una sola velocidad (`PYP2228`); sin un `I2C.start` alcanzable desde `main` el uso es un error (`PYP2225`). Con `start`, `RC3` y `RC4` quedan tomados (`PYP2150` si también se usan como E/S); `RC5` (SDO) sigue libre. Errores: `PYP2224` (importación), `PYP2226` (argumentos de `start`), `PYP2227` (método inexistente), `PYP2229` (`I2C` usado como valor).

**Verificación:** simulador de PyxPic (`crates/pyxpic-pic14/tests/i2c_modulo.rs`: bus con esclavos virtuales, ACK, START repetido, ACK ausente, lectura con ACK y NACK, registros según el reloj y reglas). **Proteus y hardware real: sin probar.** `examples/python/i2c/escaner.py` prueba las direcciones 1 a 127 y envía por la UART las que responden (el simulador lo verifica con esclavos en `0x27` y `0x50`).

### Leer de un dispositivo

La dirección se envía con el bit R/W en 1, y cada byte se pide con `read`:

```python
I2C.begin()
if I2C.write(0x41):                  # PCF8574 en 0x20, lectura
    entradas = I2C.read(ack=False)   # un solo byte: NACK
I2C.end()
```

`examples/python/i2c/leer_pcf8574.py` lee ocho interruptores de un `PCF8574` y los muestra en PORTD. Según su hoja de datos (TI, SCPS068K): dirección `0100 A2 A1 A0`, SCL de 100 kHz como máximo y los pines que se leen deben escribirse antes en alto. **Verificado solo en el simulador.** Los bytes leídos en el modelo salen de `sim.i2c.read_data`; el intérprete de la IR lee `0xFF`.

## Lo que ya funcionaba antes del módulo

`examples/python/i2c/lcd_i2c.py` maneja una pantalla HD44780 de 16x2 con la mochila PCF8574 y escribe una línea de texto en cada renglón. Usa el MSSP por medio de `pyxpic.regs` (`SSPADD`, `SSPCON`, `SSPCON2`, `SSPBUF`, `SSPSTAT`), que ya compila:

```python
from pyxpic.device import PIC16F887
from pyxpic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC

SSPADD.write(9)                  # Fosc/(4*(SSPADD+1)) = 4 MHz / 40 = 100 kHz
SSPCON.write(0x28)               # SSPEN = 1 y SSPM = 1000: I²C maestro
SSPCON2.SEN.set()                # START; el hardware baja SEN al terminar
SSPBUF.write(0x4E)               # dirección 0x27 con R/W = 0
while SSPSTAT.R_W.value() != 0:  # R/W = 1 mientras sale el byte y su ACK
    pass
```

La mochila más común: dirección `0x27` (PCF8574) o `0x3F` (PCF8574A), con `P0` = RS, `P1` = RW, `P2` = E, `P3` = luz y `P4` a `P7` = D4 a D7. Si la tuya cambia el orden de los bits, se ajustan las constantes del ejemplo. Los tiempos del HD44780 (E mayor de 450 ns, 37 µs por orden, 1,52 ms tras borrar) son los de su datasheet y el ejemplo espera más que ellos.

**Pull-ups:** el PIC no los tiene; SCL (RC3) y SDA (RC4) necesitan resistencias a VDD (4,7 kΩ típico a 100 kHz). Muchas mochilas ya las traen. El compilador no puede comprobarlo.

## El modelo del simulador

`crates/pyxpic-pic14/src/i2c_sim.rs`, con esclavos virtuales: `sim.i2c.slaves.push(0x27)` y, tras correr, `sim.i2c.events` (START, dirección con su ACK, datos con su ACK, STOP) y `sim.i2c.received_by(0x27)`.

| Qué se modela | Detalle | Página |
|---|---|---|
| `SEN`, `RSEN`, `PEN` | inician START, START repetido y STOP; el hardware los baja solo y sube `SSPIF` (`PIR1<3>`); `S` y `P` de `SSPSTAT` | 176, 178, 192 |
| Un solo evento a la vez | con el MSSP ocupado esos bits no se activan | 178 (nota 1), 189 |
| Escribir `SSPBUF` | transmite 8 bits; `R/W` y `BF` valen 1 mientras tanto | 176 |
| `ACKSTAT` | 0 si algún esclavo respondió ACK a la dirección; 1 si no | 178 |
| `RCEN`, `ACKEN` | tras una dirección de lectura con ACK, `RCEN` entrega un byte de `sim.i2c.read_data` y `ACKEN` responde según `ACKDT`; terminar sin NACK es una violación registrada | 189 |
| `WCOL` | escribir `SSPBUF` con una operación en curso lo sube y no escribe | 177, 192 |
| Modo apagado | `SEN`/`RSEN`/`PEN` o `SSPBUF` sin `SSPEN` y `SSPM = 1000` es una violación registrada | 177 |

**Tiempos (aproximados, no medidos en el chip):** el reloj de SCL es Fosc/(4 × (SSPADD + 1)), es decir, un periodo de SSPADD + 1 ciclos de instrucción; START, START repetido y STOP duran un periodo y un byte con su ACK, nueve; un byte recibido, ocho, y su ACK, uno.

**No se modela:** arbitraje y colisión de bus (`BCLIF`), alargamiento de reloj del esclavo, direcciones de 10 bits, modo esclavo, las interrupciones del MSSP ni el control de pendiente (`SMP`).

## Pruebas (`crates/pyxpic-pic14/tests/i2c.rs`)

El ejemplo corre en el simulador con un esclavo en `0x27`: todas las transacciones son START, dirección con ACK, un dato con ACK y STOP; el decodificador del HD44780 reconstruye los nibbles en cada bajada de E y encuentra el arranque a 4 bits (3, 3, 3, 2), las órdenes `0x28`, `0x0C`, `0x01`, `0x06`, `0x80` y `0xC0` y el texto de las dos líneas. Con el esclavo en otra dirección, nadie responde ACK. Además: el `ACKSTAT` de una sonda a una dirección presente y a una ausente, `WCOL`, el MSSP apagado y los tiempos según `SSPADD`.

## Siguiente

- El explorador de registros (#78), ahora que hay `I2C.read`. El escáner (#77) ya está como ejemplo.
- Probar la lectura en Proteus y en hardware (`leer_pcf8574.py`).
- Que `lcd_i2c.py` use el módulo en lugar de `pyxpic.regs`.
- Una biblioteca de LCD con `print_at` cuando el lenguaje tenga cadenas.
- Pendiente de leer: los tiempos de bus del cap. 17 para documentar la velocidad real según el reloj.
