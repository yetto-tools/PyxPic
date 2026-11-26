# I²C maestro por hardware y pantalla LCD 16x2 (MSSP)

**Estado: el módulo `pypic.periph.i2c` todavía no existe. Lo hecho es el modelo del MSSP en el simulador y un ejemplo funcional con `pypic.regs`.** El MSSP del PIC16F887 (DS40001291H, cap. 13, pp. 175-205) en modo maestro de 7 bits, solo transmisión.

**Verificación.** Simulador de PyPIC: pruebas diferenciales. **Proteus (simulación, no hardware real):** el usuario vio `lcd_i2c.hex` mostrar «Hola PyPIC» e «I2C 100 kHz» en un `LM016L` (LCD de 16x2) con un `PCF8574` como pieza separada, con A0, A1 y A2 a VDD (dirección `0x27`) y RS, RW y E en P0 a P2, D4 a D7 en P4 a P7 (captura del 2026-11-26). El componente combinado `JHD-2X16-I2C` de Proteus no mostró texto aunque el bus y los bytes eran correctos (ACK en cada transacción en el depurador I²C), así que no está confirmado que ese modelo use la misma disposición de bits. Con A0, A1 y A2 a tierra el `PCF8574` es `0x20` (se escribe `0x40`), no `0x27`. **Hardware real: sin probar.**

## Lo que ya funciona

`examples/python/i2c/lcd_i2c.py` maneja una pantalla HD44780 de 16x2 con la mochila PCF8574 y escribe una línea de texto en cada renglón. Usa el MSSP por medio de `pypic.regs` (`SSPADD`, `SSPCON`, `SSPCON2`, `SSPBUF`, `SSPSTAT`), que ya compila:

```python
from pypic.device import PIC16F887
from pypic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC

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

`crates/pypic-pic14/src/i2c_sim.rs`, con esclavos virtuales: `sim.i2c.slaves.push(0x27)` y, tras correr, `sim.i2c.events` (START, dirección con su ACK, datos con su ACK, STOP) y `sim.i2c.received_by(0x27)`.

| Qué se modela | Detalle | Página |
|---|---|---|
| `SEN`, `RSEN`, `PEN` | inician START, START repetido y STOP; el hardware los baja solo y sube `SSPIF` (`PIR1<3>`); `S` y `P` de `SSPSTAT` | 176, 178, 192 |
| Un solo evento a la vez | con el MSSP ocupado esos bits no se activan | 178 (nota 1), 189 |
| Escribir `SSPBUF` | transmite 8 bits; `R/W` y `BF` valen 1 mientras tanto | 176 |
| `ACKSTAT` | 0 si algún esclavo respondió ACK a la dirección; 1 si no | 178 |
| `WCOL` | escribir `SSPBUF` con una operación en curso lo sube y no escribe | 177, 192 |
| Modo apagado | `SEN`/`RSEN`/`PEN` o `SSPBUF` sin `SSPEN` y `SSPM = 1000` es una violación registrada | 177 |

**Tiempos (aproximados, no medidos en el chip):** el reloj de SCL es Fosc/(4 × (SSPADD + 1)), es decir, un periodo de SSPADD + 1 ciclos de instrucción; START, START repetido y STOP duran un periodo y un byte con su ACK, nueve.

**No se modela:** recepción (`RCEN`, `ACKEN`), arbitraje y colisión de bus (`BCLIF`), alargamiento de reloj del esclavo, direcciones de 10 bits, modo esclavo, las interrupciones del MSSP ni el control de pendiente (`SMP`).

## Pruebas (`crates/pypic-pic14/tests/i2c.rs`)

El ejemplo corre en el simulador con un esclavo en `0x27`: todas las transacciones son START, dirección con ACK, un dato con ACK y STOP; el decodificador del HD44780 reconstruye los nibbles en cada bajada de E y encuentra el arranque a 4 bits (3, 3, 3, 2), las órdenes `0x28`, `0x0C`, `0x01`, `0x06`, `0x80` y `0xC0` y el texto de las dos líneas. Con el esclavo en otra dirección, nadie responde ACK. Además: el `ACKSTAT` de una sonda a una dirección presente y a una ausente, `WCOL`, el MSSP apagado y los tiempos según `SSPADD`.

## Siguiente

- Recepción (`RCEN`, `ACKEN`) para leer dispositivos y registros.
- El módulo `pypic.periph.i2c` (`start`, `probe`, `write`, `read`, `stop`) y su validación contra el Device Pack; el escáner I²C (#77) y el explorador de registros (#78).
- Una biblioteca de LCD con `print_at` cuando el lenguaje tenga cadenas.
- Pendiente de leer: los tiempos de bus del cap. 17 para documentar la velocidad real según el reloj.
