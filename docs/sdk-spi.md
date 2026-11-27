# SPI maestro por hardware (`pyxpic.periph.spi`)

**Estado: implementado en modo maestro (`start`, `transfer`) y verificado en el simulador de PyxPic. Sin probar en Proteus ni en hardware.**
Usa el MSSP del PIC16F887 (DS40001291H, cap. 13, pp. 176-184).

```python
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD1
from pyxpic.periph.spi import SPI

CLOCK: Internal[4 * MHz]


def main() -> None:
    RD1.output()
    RD1.on()                          # chip select en reposo
    SPI.start(freq=1000000, mode=0)
    RD1.off()                         # selecciona el dispositivo
    SPI.transfer(0x05)                # envía; lo recibido se descarta
    estado: uint8 = SPI.transfer(0)   # envía un relleno y recibe
    RD1.on()
```

| Pin | Señal | Dirección (la pone `start`) |
|---|---|---|
| RC3 | SCK | salida |
| RC4 | SDI (entra al PIC) | entrada |
| RC5 | SDO (sale del PIC) | salida |

El **chip select** es un pin cualquiera que maneja tu programa: `start` no toca RA5 (SS), que solo sirve en modo esclavo.

| Método | Qué hace |
|---|---|
| `SPI.start(freq=N, mode=M)` | `N` es la frecuencia **máxima** de SCK (constante, en Hz): se elige FOSC/4, /16 o /64, nunca más rápido. `M` es el modo 0 a 3 (por defecto 0) |
| `SPI.transfer(b)` | envía `b` (MSb primero) y devuelve el byte recibido a la vez. Es la única operación: para solo enviar se ignora el resultado y para solo recibir se envía un relleno |

## Modos (tabla 13-1, p. 184)

| `mode` | `CKP` (reloj en reposo) | `CKE` |
|---|---|---|
| 0 | 0 (bajo) | 1 |
| 1 | 0 (bajo) | 0 |
| 2 | 1 (alto) | 1 |
| 3 | 1 (alto) | 0 |

`SMP` queda en 0: SDI se muestrea a la mitad del tiempo de dato.

## Costo

`start` apaga el MSSP, escribe `SSPSTAT`, las direcciones de RC3 a RC5 y `SSPCON`. `transfer` carga el byte en `SSPBUF`, espera `BF` en un bucle de dos instrucciones y lee `SSPBUF` (siempre, aunque se ignore el resultado, para que `BF` quede en 0). La espera dura lo que tarda el byte: 8, 32 o 128 ciclos de instrucción con FOSC/4, /16 y /64, redondeado al bucle. No hay tiempo límite: en modo maestro el reloj lo genera el propio PIC.

## Errores y avisos

| Código | Significado |
|---|---|
| `PYP2233` | importación incorrecta o dispositivo sin MSSP |
| `PYP2234` | argumentos de `start` |
| `PYP2235` | método inexistente |
| `PYP2236` | `SPI` usado como valor |
| `PYP2237` | se usa el SPI sin un `start` alcanzable desde `main` |
| `PYP2238` | frecuencia imposible (menor que FOSC/64) o dos configuraciones distintas |
| `PYP2239` | el programa usa SPI e I²C: son el mismo módulo |
| `PYP3023` | aviso: SCK real por debajo del 80 % de lo pedido |

Con `start`, RC3, RC4 y RC5 quedan tomados: usarlos además como E/S es `PYP2150`.

## Lo que no hay (todavía)

Modo esclavo, reloj de Timer2/2 (`SSPM` = 0011), `SMP` = 1, recepción por interrupción, dos configuraciones en un programa y SPI por software (`pyxpic.soft.spi`).

## Verificación

`crates/pyxpic-pic14/tests/spi_modulo.rs`: bytes enviados y recibidos, los tres divisores, los registros de cada modo, las direcciones de los pines, los diagnósticos y el ejemplo `examples/python/spi/lazo.py` (cable de RC5 a RC4; LED en RD0). El modelo (`crates/pyxpic-pic14/src/spi_sim.rs`) **no simula las formas de onda**: que cada `mode` funcione con un dispositivo real queda por comprobar en Proteus o en hardware. El intérprete de la IR no modela el bus y devuelve `0xFF`.
