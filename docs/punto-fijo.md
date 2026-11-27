# Punto fijo: `Fixed[T, n]` y `QFixed[T, n]`

**Estado: implementado.** El intermedio es de 16 o de 32 bits según lo que pida el valor y soporte el dispositivo (ver «Qué tan ancho es el intermedio»). Un valor de punto fijo es un **entero** que representa `raw / factor`. No hay `float`, ni exponente, ni normalización: suma, resta y comparación son las del entero, y el compilador pone la conversión de escala. En el PIC todo son instrucciones enteras.

| Tipo | Escala | `factor` | Para | Reescalar al multiplicar |
|---|---|---|---|---|
| `Fixed[int16, 2]` | decimal | 10ⁿ | sensores, tensiones, mediciones (`25.37`) | división entera (rutina del runtime en software: **el PIC de 8 bits no tiene división por hardware**) |
| `QFixed[int16, 8]` | binaria | 2ⁿ | PID, filtros, control de motores | desplazamiento de bits |

`T` es un entero (`uint8`, `int8`, `uint16`, `int16`, `uint32`, `int32`) y `n` la escala: en `Fixed`, el mayor `n` con `10ⁿ ≤ máximo de T` (que quepa al menos 1.0: 2 en 8 bits, 4 en 16, 9 en 32); en `QFixed`, hasta los bits de `T` (menos el signo). Ocupa lo mismo que `T`.

| Tipo | RAM | Rango |
|---|---|---|
| `Fixed[uint8, 2]` | 1 B | 0 a 2,55 |
| `Fixed[int16, 2]` | 2 B | −327,68 a 327,67 |
| `Fixed[uint16, 2]` | 2 B | 0 a 655,35 |
| `QFixed[int8, 4]` | 1 B | −8 a 7,9375 |

```python
temperature: Fixed[int16, 2] = 25.37       # raw 2537
temperature += 1.25                         # raw 2662: 26.62
level: QFixed[int8, 4] = 1.5                # raw 24
half: Fixed[uint16, 2] = 100.00
half = half / 4                             # 25.00 (un desplazamiento: sin signo y potencia de dos)
```

## Reglas

- **Constantes:** un literal o constante decimal se convierte al `raw` al compilar. En `Fixed` debe ser **exacto** (`Fixed[int16, 2] = 0.005` es `PYP2252`); en `QFixed` se redondea al paso más cercano y **avisa** (`PYP3024`: `0.1` en Q8 se guarda como `0.101562`). Fuera del rango es `PYP2253`.
- **Sin mezclas implícitas:** dos tipos o escalas distintos, o un entero con un punto fijo, es `PYP2254` (convierte con `Fixed[int16, 2](x)`).
- **Suma, resta, comparaciones, `-x`, `+=` y `-=`:** directas. `*=` y `/=` no existen en punto fijo (`x = x * y`); `%`, desplazamientos y bits tampoco (`PYP2253`).
- **Producto de dos del mismo tipo** (`voltage * current`): `Cast` al intermedio, `Mul`, reescala (`Div` entre 10ⁿ o `Shr` n) y `Cast` de vuelta. Si el resultado no cabe en el tipo, **da la vuelta como cualquier entero** (módulo 2^bits): no hay saturación.
- **Por una constante:** `x * 2` (un entero) multiplica sin reescalar; `x * 0.5`, `x * 1.25`, `x / 2`, `x / 2.5` son una fracción exacta `num/den` (ambos hasta 32767). Un divisor que es potencia de dos sobre un tipo sin signo es un desplazamiento; en los demás casos, una división entera. `x / y` entre dos puntos fijos no existe (`PYP2253`); una constante entre un punto fijo, tampoco.
- **Redondeo:** todas las divisiones **truncan hacia cero** (la de `//`); el desplazamiento solo se usa donde da lo mismo (sin signo). `QFixed * QFixed` desplaza (aritmético): en negativos redondea **hacia −∞**, no hacia cero (`−2.25` sale `−2.25`, pero `−0.0625 × 0.5` sale `−0.0625`, no `0`).
- **Conversiones explícitas:** `Fixed[int16, 2](n)` (entero → punto fijo, `n × 10ⁿ`), `Fixed[int16, 1](t)` / `QFixed[...](t)` (otra escala, fracción exacta) y `uint8(t)` (la parte entera, hacia cero). Las constantes se convierten al compilar.
- **Constantes de módulo:** `K: Const[Fixed[int16, 2]] = 1.5 * 3` no ocupa RAM; `FACTOR: Const = 0.5` sigue siendo un decimal sin tipo (`docs/decimales.md`).

## Qué tan ancho es el intermedio

Multiplicar dos valores de `n` bits da hasta `2n` bits, y subir de escala (`Fixed[int16, 3](t)`, `t * 1.5`) multiplica por una constante antes de dividir. El compilador **demuestra** para cada operación cuál es el menor intermedio (**16 o 32 bits**) en el que cabe el peor caso del tipo, dentro de lo que soporta el dispositivo (`max_int_bits` de su Device Pack: 32 en el PIC16F887):

| Operación | Intermedio |
|---|---|
| `a * b` con base de 8 bits (`Fixed[uint8, 2]`, `QFixed[int8, 4]`) | 16 bits |
| `a * b` con base de 16 bits (`Fixed[uint16, 3]`: `3.300 × 0.250 = 0.825`) | 32 bits |
| `x / 2`, `x * 2`, `uint8(x)` | 16 bits o menos: no se ensancha más que el tipo |
| `t * 1.5` o `Fixed[int16, 3](t)` con `t: Fixed[int16, 2]` | 32 bits |
| `a * b` con base de 32 bits | **`PYP2255`**: haría falta 64 bits |

- **Costo:** un intermedio de 32 bits usa las rutinas de 4 bytes del runtime (`mul32`: **1817 ciclos**, `divmod32`: 1145–2425; `docs/sdk-runtime.md`). El compilador lo **avisa una vez por programa** (`PYP3025`). Si necesitas rapidez, una base de 8 bits o un `QFixed` (que desplaza en lugar de dividir) es mucho más barato.
- **El PIC de 8 bits no tiene multiplicar ni dividir por hardware:** todo intermedio mayor que el byte es una rutina en software; el aviso no es un detalle.
- **Un dispositivo con menos ancho:** si `max_int_bits` es 16, lo que pida 32 bits es `PYP2255` con el ancho del dispositivo en el mensaje; la prueba `the_intermediate_width_follows_what_the_device_supports` lo comprueba con un perfil de 16 bits.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP2252` | Tipo mal formado (`Fixed[bool, 2]`, escala fuera de rango) o decimal no exacto en `Fixed` |
| `PYP2253` | Valor fuera de rango, o un operador que no se aplica al punto fijo |
| `PYP2254` | Tipos distintos o un entero sin convertir |
| `PYP2255` | El intermedio no cabe en el ancho que soporta el dispositivo |
| `PYP3025` | Aviso: la operación usa un intermedio de 32 bits (rutinas de 4 bytes) |
| `PYP2256` | La fracción constante es demasiado grande (numerador o denominador > 32767) |
| `PYP3024` | Aviso: una constante `QFixed` no es exacta y se redondea |

**Verificación:** `crates/pyxpic-pic14/tests/fixed.rs`: cada operación corre en el intérprete de la IR y en el simulador del PIC y deben dar los mismos globales, con los valores exactos esperados (`1.10 × 0.50 = 0.55`, `−0.50 × 1.20 = −0.60`, Q4 `1.5 × 1.5 = 2.25`, conversiones, fracciones, comparaciones) y todos los rechazos con su código. **No probado en hardware.**
