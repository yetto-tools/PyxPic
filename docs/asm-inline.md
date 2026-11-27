# Ensamblador en línea (`@asm_function`)

**Estado: implementado (#42, primera parte).** Cuando el lenguaje no alcanza, una función puede escribirse **entera** en ensamblador del PIC16 clásico, con las 35 instrucciones y el mismo ensamblador que `pyxpic asm`, con la **sintaxis de pic-as** (MPLAB XC8 PIC Assembler,
[asm-pic14.md](asm-pic14.md)). Es la salida de emergencia que hace que PyxPic no limite lo que se puede hacer con un PIC.

```python
from pyxpic.device import PIC16F887
from pyxpic import uint8, asm_function

swapped: uint8 = 0


@asm_function
def swap_nibbles(x: uint8) -> uint8:
    """
    SWAPF x,W       ; los parametros y los globales se usan por su nombre
    RETURN          ; el resultado de 8 bits vuelve en W
    """


def main() -> None:
    global swapped
    swapped = swap_nibbles(0xA5)
```

## Por qué una función y no un fragmento suelto

Se adoptó lo útil de otros lenguajes y se descartó lo incómodo:

| Idea | Origen | Decisión |
|---|---|---|
| Ensamblador dentro de una **función con interfaz declarada** (parámetros y retorno tipados) | Rust (`naked`), Zig | Adoptada |
| **Operandos con nombre**, sin números ni cadenas de restricciones | Rust | Adoptada: `SWAPF x,W` |
| El compilador **comprueba** lo que el bloque hace, en vez de fiarse de lo declarado | Rust | Adoptada: vuelta al banco 0, final de función, saltos dentro de la función |
| Etiquetas **locales** a cada función | GNU as | Adoptada |
| Fragmentos sueltos (`asm(...)`) dentro de otra función | C, GCC | **Descartada**: el compilador no sabe en qué banco está ni qué hay en W en ese punto |
| Cadenas de restricciones (`"=r"`) y *clobbers* a mano | GCC | **Descartada** |

## Interfaz con el resto del programa

- El cuerpo es **una sola cadena** (normalmente de tres comillas). Es la única cadena que admite el lenguaje.
- **Parámetros y retorno** de hasta 16 bits (`uint8`, `int8`, `uint16`, `int16`, `bool`). Cada parámetro es un símbolo con la dirección de su byte bajo; un `uint16` ocupa `x` (bajo) y `x+1` (alto).
- **Resultado**: el byte bajo en `W`; en un resultado de 16 bits, el alto se escribe antes en `RETV` (`0x7C`).
- **Globales**: por su nombre (`hits`, `marker`...); las constantes `Const` valen su valor.
- **Registros y bits predefinidos**: los mismos que da `#include <xc.inc>` en `pyxpic asm`, desde `registers.json` del Device Pack (73 registros del PIC16F887).
  Cada registro vale su **dirección completa** (`TRISD` = `0x88`), así que fuera del banco 0 se usa `BANKMASK(TRISD)` tras cambiar el banco a mano con
  `BSF STATUS,STATUS_RP0_POSN`; cada campo tiene `REGISTRO_CAMPO_POSN` (`STATUS_Z_POSN`, `ADCON0_GO_DONE_POSN`...) y los campos únicos de más de una letra son macros
  (`BSF GO_DONE` = `BSF ADCON0,1`). Un parámetro o global con uno de esos nombres da error `ASM005` solo si el bloque lo usa.
- **Etiquetas** locales a la función, solo como destino de `CALL`/`GOTO` (con `$` y aritmética: `GOTO $+1`). `EQU` define constantes locales.

## Lo que el compilador comprueba

| Código | Regla |
|---|---|
| `PYP2110` | El cuerpo de una `@asm_function` es una única cadena |
| `PYP2111` | No se alcanza desde una `@interrupt` (comparte memoria estática con el contexto principal) |
| `PYP2112` | Parámetros y retorno de hasta 16 bits |
| `PYP2113` | No se combina con `@interrupt` ni `@init_device` |
| `ASM007` | Sin `PSECT`/`ORG`/`DB`/`DW`/`DS`/`CONFIG` ni `BANKSEL`/`PAGESEL`/`fcall`/`ljmp` (el compilador coloca la función y gestiona el banco y la página; una tabla se escribe con `RETLW`), sin `RETFIE`; debe terminar en `RETURN`, `RETLW` o `GOTO`; **no puede volver con `RP0`/`RP1` puestos** |
| `ASM006` | Un salto fuera de la función |
| `ASM001`–`ASM005` | Los mismos que el ensamblador textual |

Los errores llevan la **línea del archivo `.py`**. La comprobación de `RP0`/`RP1` sigue el orden del texto (no analiza ramas): es una red de seguridad, no una prueba formal.

## Límites actuales

- Solo se llama **desde el contexto principal**, no desde la interrupción.
- Dentro de la función `CALL` solo salta a etiquetas propias (cada `CALL` usa un nivel de la pila de hardware que el análisis de profundidad aún no cuenta). Llamar a otras funciones desde el ensamblador está pendiente.
- El intérprete de referencia de la IR no ejecuta estas funciones (`Trap::AsmFunction`): solo existen en el dispositivo; se prueban con el simulador del núcleo PIC14.
- No hay bloques de ensamblador sueltos dentro de una función ni acceso a variables locales (las funciones de ensamblador solo tienen parámetros).
