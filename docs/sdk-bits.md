# SDK: bits y acarreo (`pypic.bits`)

Intrínsecos que mapean **1:1 a una instrucción** del PIC16 (principio del proyecto: ensamblador de alto nivel, `docs/principios-de-diseno.md`), con el costo
en ciclos a la vista en `--listing`. Sirven para portar rutinas de ensamblador: desplazamientos multibyte, CRC, rotaciones, banderas de bit.

```python
from pypic.bits import swap, rol_c, ror_c, clear_carry, set_carry, carry, bit_set, bit_clear, bit_test

clear_carry()        # BCF STATUS,C
lo = rol_c(lo)       # RLF lo,F       (el bit 7 pasa a C y C entra por el bit 0)
hi = rol_c(hi)       # RLF hi,F       (encadena el acarreo de lo)
sale = carry()       # el valor de C como bool
```

| Función | Instrucción | Ciclos | Notas |
|---|---|---|---|
| `swap(x)` → `uint8` | `SWAPF x,W` (o `x,F` si es `x = swap(x)`) | 1 (+1 si se guarda) | `x` puede ser una expresión uint8 o una constante |
| `rol_c(x)` → `uint8` | `RLF x,F` o `RLF x,W` + `MOVWF` | 1 (+1) | `x` es una variable `uint8`; usa el acarreo de entrada |
| `ror_c(x)` → `uint8` | `RRF` igual | 1 (+1) | |
| `clear_carry()` / `set_carry()` | `BCF`/`BSF STATUS,C` | 1 | |
| `carry()` → `bool` | `CLRF d` · `BTFSC STATUS,C` · `INCF d,F` | 3 | |
| `bit_set(x, n)` / `bit_clear(x, n)` | `BSF`/`BCF x,n` | 1 | `x` variable de hasta 16 bits; `n` constante (en un `uint16`, `n ≥ 8` toca el byte alto) |
| `bit_test(x, n)` → `bool` | `CLRF d` · `BTFSC x,n` · `INCF d,F` | 3 | |

## La regla del acarreo

El compilador no promete qué deja en `C` una suma o una comparación, así que **el acarreo solo se puede usar donde está definido**, y eso lo comprueba el compilador:

- `rol_c`, `ror_c` y `carry()` necesitan que **la sentencia anterior** haya sido `clear_carry()`, `set_carry()`, `rol_c` o `ror_c`
  (como expresión completa, `y = rol_c(x)`), y deben ser **la expresión completa** de su sentencia (`y = rol_c(x) + 1` no se admite).
- Un `if`, un bucle, otra sentencia o el cambio de función **pierden** el acarreo: dentro de un bucle se vuelve a empezar con `clear_carry()`.
- El argumento de `rol_c`/`ror_c` es una **variable**, no una expresión (calcularla alteraría `C` antes de rotar).

| Error | Qué dice |
|---|---|
| `PYP2140` | `pypic.bits` no exporta ese nombre |
| `PYP2141` | Aridad o tipo (`rol_c` rota un `uint8`; `bit_*` hasta 16 bits) |
| `PYP2142` | El acarreo no está definido aquí / no es la expresión completa |
| `PYP2143` | El argumento debe ser una variable (no una expresión ni una constante `Const`) |
| `PYP2144` | El número de bit debe ser constante y caber en el ancho de la variable |

## Lo que no está (y por qué)

`zero()` y `digit_carry()` (las banderas Z y DC) **no se ofrecen**: tras una suma o una comparación generadas por el compilador, qué instrucción fue la última
y si dejó Z o DC no es parte del contrato del lenguaje, y exponerlo sería prometer algo que cambia con cada optimización. Para banderas del núcleo en
código de usuario existe `pypic.regs` (`STATUS.Z.value()`, sin garantía de contexto) y, con garantía total, `@asm_function`. Queda abierto en #59.

## Verificación

`crates/pypic-pic14/tests/bits.rs`: prueba diferencial entre el intérprete de la IR (que modela C exactamente) y el simulador del núcleo — desplazamiento de
16 bits encadenado, rotación a la derecha con `C` puesto, `swap`, bits en `uint8`/`uint16`/locales, un bucle que multiplica por 2^12 — más todos los rechazos de arriba.
Ejemplo: `examples/python/registros/shift16.py`.
