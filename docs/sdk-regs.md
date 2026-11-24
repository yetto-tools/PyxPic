# SDK: registros y bits (`pypic.regs`)

Todo registro de funciones especiales (SFR) del dispositivo se importa por nombre, validado contra `registers.json` del Device Pack, y se usa con
métodos, igual que los pines y los timers. Es el equivalente de `MOVF`, `MOVWF`, `BSF`, `BCF` y `XORWF` del ensamblador, con el banco gestionado
por el compilador pero **visible** en el costo (`--listing`, `--timing`).

```python
from pypic.device import PIC16F887
from pypic.regs import OPTION_REG, WPUB, STATUS

OPTION_REG.RBPU.clear()        # BCF OPTION_REG,RBPU
WPUB.write(0b0000_0011)        # MOVLW / MOVWF WPUB
v = OPTION_REG.read()          # MOVF OPTION_REG,W
if STATUS.Z.value() == 1:      # el bit, como uint8 (0 o 1)
    ...
```

## Una sola forma por operación

| Operación | Código | Instrucciones | Ciclos |
|---|---|---|---|
| Leer el byte | `REG.read()` → `uint8` | `MOVF f,W` + guardar | 1 + banco |
| Escribir el byte | `REG.write(v)` (`v` es `uint8`) | cargar `W` + `MOVWF f` | 1 + banco |
| Poner un bit | `REG.BIT.set()` | `BSF f,b` | 1 + banco |
| Borrar un bit | `REG.BIT.clear()` | `BCF f,b` | 1 + banco |
| Invertir un bit | `REG.BIT.toggle()` | `MOVLW m` + `XORWF f,F` | 2 + banco |
| Leer un bit | `REG.BIT.value()` → `uint8` (0 o 1) | `MOVF` + `ANDLW` + `BTFSS`/`INCF` | 5 + banco |

**Banco:** si el registro no está en el banco 0 se añaden `BSF/BCF STATUS,RP0/RP1` antes y después (2 ciclos por cada bit de banco; el banco 3 son 4).
El código del programa siempre corre en el banco 0 (ABI, `docs/abi-pic14.md`), así que el compilador lo restablece tras cada acceso.

## Validación

| Se rechaza | Error |
|---|---|
| Importar un registro que el dispositivo no tiene (lista los que sí) | `PYP2130` |
| Importar `INDF`/`EECON2` (no son físicos) o un registro que comparte dirección con otro (`SSPMSK`/`SSPADD`) | `PYP2130` |
| Usar un bit que el registro no tiene (lista sus bits) | `PYP2133` |
| Un método que no existe (`TRISB.on()`) | `PYP2134` |
| `STATUS.write`, `PCL.write`, `PCLATH.write`: los usa el compilador | `PYP2135` |
| `set/clear/toggle` de `RP0`, `RP1` o `IRP`: eligen el banco, lo gestiona el compilador | `PYP2135` |
| Usar el registro como valor (`x = TRISB`) | `PYP2136` |

Para lo que el compilador reserva (cambiar de banco a mano, escribir `PCL`...) la salida es `@asm_function` (`docs/asm-inline.md`).

## Alcance y límites

- Un registro se identifica por su **primera dirección** en `registers.json` (p. ej. `INTCON` en `0x0B`); las demás aparecen como espejos.
- No se comprueba si el programa y un periférico declarado (`TMR2`, pines...) escriben el mismo registro: es el aviso de conflictos de #54.
- `read()` de un puerto lee el pin; el latch se lee con el registro `PORTx` de la misma forma, como en ensamblador.

## Pines que toma un periférico (`PYP2150`, `PYP3010`)

Encender un periférico puede quitarle un pin al GPIO: con `RCSTA.SPEN` en 1, RC6 y RC7 pasan a ser `TX` y `RX`. `peripherals.json` dice qué bit reclama qué función
(`claimed_by`: `SPEN` → `TX`/`RX`/`CK`/`DT`, `SSPEN` → `SCK`/`SDI`/`SDO`/`SCL`/`SDA`, `C1OE`/`C2OE` → `C1OUT`/`C2OUT`, `T0CS` → `T0CKI`, `TMR1CS` → `T1CKI`,
`T1OSCEN` → `T1OSI`/`T1OSO`, `TMR1GE` → `T1G`), y el compilador lo aplica a lo que el programa enciende con `REG.BIT.set()` o con `REG.write(constante)`:

| Código | Regla |
|---|---|
| `PYP2150` | Un pin que un periférico encendido toma también se importa como E/S (`from pypic.periph.gpio import RC6`): el mensaje nombra el pin, la función y el periférico |
| `PYP3010` (aviso) | Dos periféricos distintos reclaman el mismo pin (`T0CKI` de Timer0 y `C1OUT` del comparador en RA4) |
| `PYP2125` | Lo mismo que `PYP2150` para el reloj externo de un timer iniciado con `TMRx.start(...)` |

No se reclama `SS` del MSSP (en maestro puede ser E/S). **No cubierto:** los modos SPI/I²C del MSSP (hace falta el campo `SSPM` y su módulo, #45) y el aviso de un pin
analógico usado como digital: el compilador ya lo pasa a digital (borra su bit de `ANSEL`/`ANSELH`) en `pin.input()`/`pin.output()`.

## Verificación

`crates/pypic-pic14/tests/regs.rs`: prueba diferencial entre el intérprete de la IR (que modela los registros como memoria plana) y el simulador del
núcleo sobre registros de los cuatro bancos, composición con expresiones y bucles, vuelta al banco 0 y todos los rechazos de arriba.
Ejemplo: `examples/registers.py`.
