# SDK: EEPROM de datos (`pypic.periph.eeprom`)

**Estado: implementado (tercer módulo de #45, solo la EEPROM de datos).** Los 256 bytes de EEPROM de datos del PIC16F887 (DS40001291H, cap. 10, pp. 110-113 y 119-120; parámetros D120-D124 en la p.246). La lectura y escritura de la **memoria de programa** (flash) no están: es `flash` de #46.

```python
from pypic.periph.eeprom import EEPROM

CLOCK: Internal[4 * MHz]

v: uint8 = EEPROM.read(0)        # un byte; una EEPROM virgen vale 0xFF
EEPROM.write(0, v + 1)           # borra y escribe el byte; vuelve cuando terminó (hasta 6 ms)
```

## Operaciones

| Operación | Qué hace | Ciclos |
|---|---|---|
| `EEPROM.read(a)` → `uint8` | `EEADR = a`, `EEPGD = 0`, `RD = 1` y lee `EEDAT` (el dato está al ciclo siguiente, 10.1.2) | 14 (`z = EEPROM.read(a)`, con la dirección constante o variable) |
| `EEPROM.write(a, v)` | La secuencia completa de escritura y **espera a que termine** | `start_write` + la espera: **6 031** a 4 MHz (la espera es el tiempo de escritura, 6 ms = 6 000 ciclos) |
| `EEPROM.start_write(a, v)` | La misma secuencia **sin esperar**: la CPU sigue mientras el chip escribe | 29 (con constantes) |
| `EEPROM.busy()` → `bool` | Hay una escritura en curso (`WR = 1`) | 13 (`if EEPROM.busy(): ...`, con la ramificación) |
| `EEPROM.failed()` → `bool` | La última escritura se interrumpió (`WRERR = 1`) | 13 (igual) |

`write` **es exactamente** `start_write` más esperar a que `WR` baje, no un sinónimo: `start_write` y `busy()` dan el control del ensamblador (hacer otra cosa durante los ~6 ms) y `write` es la forma corta. La dirección y el valor son `uint8`; el costo no depende de la dirección ni del valor. Costos medidos en el simulador de `crates/pypic-pic14/tests/eeprom.rs`.

## La secuencia de escritura, paso a paso (10.1.3, p.113)

`start_write` hace lo del Ejemplo 10-2 del datasheet:

1. `EEADR = a` y `EEDAT = v`;
2. `EEIF = 0` y `WRERR = 0` (la escritura anterior ya se revisó con `failed()`), `EEPGD = 0` (memoria de datos) y `WREN = 1`;
3. **interrupciones fuera**: `GIE = 0` y se comprueba que haya quedado en 0 (AN576, como el ejemplo del datasheet);
4. `55h` y luego `AAh` a `EECON2`, y `WR = 1`;
5. `GIE = 1` otra vez y **`WREN = 0` enseguida**: bajar `WREN` no afecta a la escritura en curso (p.113) y deja puesta la protección contra escrituras accidentales (10.4).

**`GIE` solo se vuelve a habilitar si estaba habilitado.** El ejemplo del datasheet hace `BSF INTCON,GIE` siempre; aquí, si `GIE` ya estaba en 0 (dentro de una interrupción, o con las interrupciones apagadas a propósito) se hace la misma secuencia **sin tocar `GIE`**: una escritura nunca enciende las interrupciones de un programa que las tenía apagadas. Las dos rutas están duplicadas en el código (7 instrucciones) porque no hay un registro libre para guardar el estado de `GIE`.

Al terminar el ciclo el hardware baja `WR` y sube `EEIF` (`PIR2<4>`); `EEIF` **no lo baja el hardware**: lo baja el siguiente `write`/`start_write` al empezar, o tu programa (`PIR2.EEIF.clear()` con `pypic.regs`).

## El tiempo de escritura y el desgaste

- **Tiempo (D122):** 5 ms típico, **6 ms máximo**. `write` espera sondeando `WR`, así que el tiempo real es el del chip (no un retardo fijo); el simulador usa los 6 ms del máximo (`Sim::eeprom_write_ns`).
- **Resistencia (D120):** **100 000 escrituras mínimas por byte** (1 000 000 típicas) de −40 a +85 °C, y 10 000 mínimas de +85 a +125 °C. **Retención (D123):** 40 años.
- **Refresco (D124 y 10.3.1, p.119):** el total de escrituras de todo el arreglo antes de refrescar es de 1 000 000 (10 000 000 típico). Si unos bytes cambian mucho y otros casi nunca, hay que reescribir el arreglo de vez en cuando. **PyPIC no cuenta ni reparte el desgaste**: eso lo decide el programa; guarda en la EEPROM lo que cambia a menudo y deja las constantes y las calibraciones en la memoria de programa (`Rom`).
- **Alimentación (D121):** la lectura y la escritura valen para toda la tensión de operación del chip.
- **`write` no limpia el WDT.** Si el perro guardián está encendido y su periodo es corto, la espera de 6 ms puede reiniciar el chip a mitad de la escritura (ver `WRERR`).

## `failed()` y los reinicios a mitad de una escritura

Si `MCLR`, el WDT o el *brown-out* reinician el chip mientras se escribe, la escritura se interrumpe y **`WRERR` queda en 1** (10.1.1, p.110): el byte conserva su valor anterior en el simulador, pero **en un chip real el contenido de esa posición no está garantizado**; el datasheet dice que se compruebe `WRERR` y se reescriba. Por eso `failed()` existe: tras un arranque, `if EEPROM.failed():` y se vuelve a escribir. Cualquier `write`/`start_write` baja `WRERR` al empezar, así que **revisa `failed()` antes de escribir**.

## Verificar lo escrito (10.3, p.119)

El datasheet recomienda verificar con una lectura (Ejemplo 10-5). `write` no lo hace solo, para que el costo se vea:

```python
EEPROM.write(a, v)
if EEPROM.read(a) != v:
    ...                    # la escritura falló: reintenta o avisa
```

## Lo que NO hace

- **Una interrupción que también use la EEPROM** (`EEADR`, `EEDAT` y `EECON1` son compartidos): si `main` está a mitad de `read` y la interrupción lee o escribe, el resultado es incierto. Usa la EEPROM desde un solo contexto, o apaga las interrupciones alrededor.
- **`write` dentro de una interrupción** funciona (una prueba lo comprueba: `GIE` sigue en 0 dentro y el `RETFIE` lo habilita), pero la espera de hasta 6 ms ocupa la CPU y retrasa las demás interrupciones: usa `start_write` y consulta `busy()` desde `main`.
- **Bloques, cadenas, estructuras o tablas de bytes** (`read_into(buf)`, `write_all(...)`): hoy es un byte por llamada; con arreglos (`docs/sdk-arreglos.md`) el bucle lo escribe el programa y su costo queda visible.
- **La memoria de programa** (lectura y escritura de la flash, `EEPGD = 1`): es el módulo `flash` de #46.
- **Protección contra copia** (`CPD` en la palabra de configuración): se configura en `CONFIG`, no aquí.

## Reglas

| Código | Regla |
|---|---|
| `PYP2210` | `pypic.periph.eeprom` no exporta ese nombre (`EEPROM`) o el dispositivo no trae el módulo (`EEPROM` de `peripherals.json`) |
| `PYP2211` | Argumentos con nombre (los métodos reciben solo posicionales) |
| `PYP2212` | Método que no existe (`read`, `write`, `start_write`, `busy`, `failed`) |
| `PYP2213` | `EEPROM` usado como valor |
| `PYP2055` | Número de argumentos equivocado |
| `PYP2041` | Dirección o valor que no es `uint8` (convierte con `uint8(...)`); una constante fuera de 0..255 no cabe |

## Verificación

El simulador del núcleo modela la EEPROM: 256 bytes que **sobreviven a los reinicios** (empiezan en 0xFF), `RD` que lee `EEDAT` desde `EEADR` y se baja solo, `WR` que **solo inicia la escritura si `EEPGD = 0`, `WREN = 1` y se hizo la secuencia `55h`, `AAh`** (`EECON2` lee 0 y no es físico), que dura `eeprom_write_ns` (6 ms por defecto), que no se puede bajar por programa y que al terminar baja `WR`, sube `EEIF` y escribe el byte; un reinicio a mitad de una escritura sube `WRERR`. Y **vigila el contrato**, anotándolo en `Sim::eeprom_violations`: `WR = 1` sin la secuencia o con `WREN = 0`, una interrupción entre `55h` y `WR`, y la lectura de la memoria de programa (no modelada).

`crates/pypic-pic14/tests/eeprom.rs` (15 pruebas) comprueba: lectura y escritura contra el **intérprete de referencia** (incluidas las direcciones 0 y 255 y un índice variable); las **256 direcciones** con un valor distinto cada una; que el contenido **sobrevive a un reinicio** y que una variable de RAM no; que `write` espera el tiempo de escritura (entre 6 000 y 6 040 ciclos a 4 MHz) y `start_write` no (29 ciclos); `busy()` verdadero justo después de `start_write`, falso al terminar, con `EEIF` y `WR`; `WREN` en 0 en reposo; que un `WR` **sin la secuencia** o con `WREN = 0` no escribe y el simulador lo anota; que `GIE` queda como estaba (con 1 y con 0); que **una interrupción tras cada una de las primeras 70 instrucciones** de `main` (con `GIE = 1`) nunca rompe la secuencia ni deja `GIE` apagado; que una interrupción forzada con `GIE = 0` entre `55h` y `WR` **sí** se detecta; `WRERR` tras un reinicio a mitad de la escritura (y que `failed()` lo informa); todas las reglas; y los costos. Ejemplo: `examples/eeprom_arranques.py` (un contador de arranques que sobrevive al apagado y parpadea RD0 esa cantidad de veces).

**Qué no demuestra:** que el simulador y el compilador usen el mismo modelo no prueba el chip. **No se ha probado en Proteus ni en un PIC real**; el desgaste, la retención y el tiempo de escritura real son los del datasheet, no los medidos. En un chip real, además, una escritura a mitad de una caída de alimentación puede dejar el byte indeterminado: el simulador solo modela el reinicio.
