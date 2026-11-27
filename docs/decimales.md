# Decimales en compilación

**Estado: implementado (paso 1 del punto fijo).** Un literal con punto decimal o exponente (`0.5`, `2.54`, `1.5e-3`, `1e3`) es un **decimal exacto**: un racional `num/den` de enteros de 128 bits. No hay `float`: ni punto flotante, ni RAM, ni una sola instrucción en el PIC. Los decimales solo existen al compilar; un valor de ejecución con un decimal necesita punto fijo (`Fixed` / `QFixed`, siguiente paso).

```python
FACTOR: Const = 0.5                     # constante sin tipo: el valor decide (aquí, un decimal)
RESULT: Const[uint8] = 100 * FACTOR     # 50 -> el compilador genera MOVLW 50
```

## Reglas

- **Exactitud:** `0.1 + 0.2` es exactamente `0.3` y `1.0 / 3 * 3` es exactamente `1` (con `float` no lo serían). Las cuatro operaciones `+ - * /` entre decimales y enteros constantes se hacen con aritmética racional.
- **`/` solo para decimales.** La división entera sigue siendo `//` (`3 // 4` es 0). `/` con dos enteros es el error `PYP1022` (el remedio está en el mensaje); con un decimal es la división exacta: `3.0 / 4` es `0.75`. Y al revés, `//` con un decimal es `PYP2251`. Un literal con punto es siempre decimal (`1.0 / 4` es `0.25`, no la división entera `0`).
- **Un decimal con valor entero vale como entero donde se pide uno:** `RESULT: Const[uint8] = 100 * 0.5` guarda 50, pero `RESULT: Const[uint8] = 0.5` es el error `PYP2041` («un decimal no entero solo existe al compilar»). **No se redondea nunca en silencio.**
- **`Const` sin tipo** (`FACTOR: Const = 0.5`, `N: Const = 40 + 2`): una constante de módulo cuyo tipo da el valor. No ocupa RAM ni código; un valor no constante es `PYP2011`, y declararla dos veces `PYP2009`.
- **Con un valor de ejecución:** `x = x * 0.5` es el error `PYP2251` («hace falta un tipo de punto fijo»): el compilador no emula coma flotante ni convierte un decimal a un entero por su cuenta.
- **Operadores:** solo `+ - * /` y el signo; `%`, desplazamientos y bits con un decimal son `PYP2251`. Un resultado demasiado grande para 128 bits es `PYP2250`.
- **Unidades:** `11.0592MHz` sigue siendo un literal con unidad (`docs/sdk-reloj.md`); un decimal sin unidad puede multiplicar cualquier constante (`11.0592 * 1000000`).

## Costo en el PIC

Cero: la prueba `a_constant_decimal_costs_nothing_in_the_pic` compila un programa con `FACTOR: Const = 0.5` y `LIMIT: Const[uint8] = 100 * FACTOR` y comprueba que genera **las mismas palabras de programa** que `LIMIT: Const[uint8] = 50`.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP1022` | `/` entre enteros (usa `//`) |
| `PYP2041` | Un decimal no entero donde se pide un entero |
| `PYP2250` | Decimal inválido o demasiado grande |
| `PYP2251` | Un decimal con un valor de ejecución, con `//` o con un operador que no admite |

**Verificación:** `crates/pyxpic-pic14/tests/decimal.rs`. **Siguiente:** `Fixed[T, n]` (escala decimal 10ⁿ) y `QFixed[T, n]` (escala binaria 2ⁿ) con suma, resta y multiplicación con intermedio ancho; después, optimizaciones algebraicas seguras.
