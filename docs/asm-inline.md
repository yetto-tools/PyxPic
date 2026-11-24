# Ensamblador en línea (`@asm_function`)

**Estado: implementado (#42, primera parte).** Cuando el lenguaje no alcanza, una función puede escribirse **entera** en ensamblador del PIC16 clásico, con las 35 instrucciones y el mismo ensamblador que `pypic asm`
([asm-pic14.md](asm-pic14.md)). Es la salida de emergencia que hace que PyPIC no limite lo que se puede hacer con un PIC.

```python
from pypic.device import PIC16F887
from pypic import u8, asm_function

swapped: u8 = 0


@asm_function
def swap_nibbles(x: u8) -> u8:
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
- **Parámetros y retorno** de hasta 16 bits (`u8`, `i8`, `u16`, `i16`, `bool`). Cada parámetro es un símbolo con la dirección de su byte bajo; un `u16` ocupa `x` (bajo) y `x+1` (alto).
- **Resultado**: el byte bajo en `W`; en un resultado de 16 bits, el alto se escribe antes en `RETV` (`0x7C`).
- **Globales**: por su nombre (`hits`, `marker`...); las constantes `Final` valen su valor.
- **Registros predefinidos** (desplazamiento de 7 bits, el banco lo eliges tú con `RP0`/`RP1`): `INDF TMR0 OPTION_REG PCL STATUS FSR PORTA..PORTE TRISA..TRISE ANSEL ANSELH PCLATH INTCON`
  y los bits `C DC Z RP0 RP1 IRP`. Verificados con el DS40001291H; el mapa completo llegará con el Device Pack (#22). Un parámetro o global con uno de esos nombres da error `ASM005`.
- **Etiquetas** locales a la función, solo como destino de `CALL`/`GOTO` (con `$` y aritmética: `GOTO $+1`). `EQU` define constantes locales.

## Lo que el compilador comprueba

| Código | Regla |
|---|---|
| `PYP2110` | El cuerpo de una `@asm_function` es una única cadena |
| `PYP2111` | No se alcanza desde una `@interrupt` (comparte memoria estática con el contexto principal) |
| `PYP2112` | Parámetros y retorno de hasta 16 bits |
| `PYP2113` | No se combina con `@interrupt` ni `@init_device` |
| `ASM007` | Sin `ORG`/`DW` (el compilador coloca la función; una tabla se escribe con `RETLW`), sin `RETFIE`; debe terminar en `RETURN`, `RETLW` o `GOTO`; **no puede volver con `RP0`/`RP1` puestos** |
| `ASM006` | Un salto fuera de la función |
| `ASM001`–`ASM005` | Los mismos que el ensamblador textual |

Los errores llevan la **línea del archivo `.py`**. La comprobación de `RP0`/`RP1` sigue el orden del texto (no analiza ramas): es una red de seguridad, no una prueba formal.

## Límites actuales

- Solo se llama **desde el contexto principal**, no desde la interrupción.
- Dentro de la función `CALL` solo salta a etiquetas propias (cada `CALL` usa un nivel de la pila de hardware que el análisis de profundidad aún no cuenta). Llamar a otras funciones desde el ensamblador está pendiente.
- El intérprete de referencia de la IR no ejecuta estas funciones (`Trap::AsmFunction`): solo existen en el dispositivo; se prueban con el simulador del núcleo PIC14.
- No hay bloques de ensamblador sueltos dentro de una función ni acceso a variables locales (las funciones de ensamblador solo tienen parámetros).
