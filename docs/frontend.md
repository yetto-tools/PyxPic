# Frontend del compilador (`pypic-frontend`)

**Estado: implementado (primer corte).** Parseo + validacion del subconjunto + tipos + constantes.
No genera IR ni codigo todavia. Escrito en Rust; Python es solo la sintaxis del lenguaje fuente.
Todo corre en el host: no hay interprete ni VM de Python en el MCU
(ver `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).

```
fuente .py -> lexer -> parser (AST propio) -> check (tipos, constantes, reglas) -> Program tipado
```

## Independencia del ancho del MCU

PyPIC no es "solo 8 bits": cubre cada MCU PIC. El lenguaje tiene enteros de ancho fijo
(`u8 i8 u16 i16 u32 i32`) y `bool`; **no hay `int` nativo ni ancho de palabra asumido**.
Que tipos admite un destino lo decide `TargetProfile` (mas adelante lo llenara el Device Pack):

- `native_bits`: ancho nativo de la CPU (8 en PIC16, 16 en PIC24/dsPIC, 32 en PIC32...).
- `max_int_bits`: ancho maximo admitido, nativo o emulado.

**Decision #1:** un tipo mas ancho que `native_bits` pero dentro de `max_int_bits` **se permite** y se emula con rutinas
nativas de software; el compilador emite una advertencia de coste (`PYP3001`, una vez por tipo). Por encima de
`max_int_bits` es error (`PYP2030`). Sin perfil, el destino es `generic` (32 bits nativos).

## Subconjunto soportado

- Nivel de modulo: `x: u8 = 0`, `K: Final[u8] = 5` (constante), funciones, `from pypic import ...`.
- Funciones: parametros y retorno tipados (`-> None` si no devuelve), `@interrupt` (una sola, sin parametros).
- Sentencias: asignacion, `+= -= *= //= %= &= |= ^= <<= >>=`, `if/elif/else`, `while`,
  `for i in range(...)`, `return`, `break`, `continue`, `pass`, `global`, llamadas.
- Expresiones: enteros (dec/hex/bin/octal), `True/False`, aritmetica, bit a bit, desplazamientos,
  comparaciones, `and/or/not`, llamadas, conversiones `u8(x)`.
- `u8 i8 u16 i16 u32 i32 bool Final interrupt` estan predeclarados; `from pypic import ...` es opcional
  (mantiene el archivo valido para otros editores).

Todo lo demas se rechaza con un diagnostico especifico: clases, listas/tuplas, cadenas, floats, `/`, `**`,
lambdas, `try`, `import` general, atributos, indexado, argumentos con nombre, comparaciones encadenadas, etc.

## Semantica fijada (confirmada, decision #2)

| Tema | Regla |
|---|---|
| Literales | Sin tipo propio; toman el del contexto si su valor cabe (`PYP2040` si no). |
| Conversiones | Siempre explicitas: `u16(a) + b`. Mezclar tipos es `PYP2047`. |
| Overflow en ejecucion | Modular (wrap-around) en el ancho del tipo. |
| Overflow constante | Error (`PYP2045`/`PYP2040`), nunca truncado en silencio. |
| Cast explicito | Trunca a proposito: `u8(u16(300)) == 44`. |
| `//` y `%` | Truncan hacia cero (como C), **no** como Python. `/` no existe. |
| Condiciones | Deben ser `bool` (sin truthiness): `if x != 0`. |
| Variables | Siempre inicializadas al declararse; las globales con constante. Declarar con tipo antes de asignar. |
| Globales | Modificarlas en una funcion exige `global x`. Un local no puede ocultar una global. |
| `for` | Solo `range(...)`; el paso debe ser constante distinto de 0; si todo es literal, el tipo es el menor que cabe. |
| Recursion | Prohibida (memoria estatica, pila de hardware limitada): `PYP2013`. |
| ISR vs main | **Decision #3.** Una funcion alcanzable desde `main` y desde la `@interrupt` es error (`PYP2060`): su memoria estatica seria compartida y la ISR podria corromperla. Se escribe una version por contexto. El contexto principal es `main`; si no existe, toda funcion sin llamadores que no sea la ISR. Compartir *globales* si esta permitido. |
| Pila de retorno | Si el destino declara `stack_levels` (8 en PIC16), el peor caso `1 + prof(main) + 1 + prof(ISR)` no puede exceder el limite: `PYP2061` (ver `docs/abi-pic14.md`). |
| Ensanchamiento | Sin conversion implicita ni siquiera sin perdida (`u8` + `u16` es `PYP2047`). |
| Desplazamientos | Cantidad literal `0..bits-1` o entero sin signo; el operando izquierdo necesita tipo. |

## Declaración del dispositivo, reloj y arranque

Un programa dice **a qué PIC está orientado** en su primera línea, y de ahí salen todas las validaciones (pines, tipos, pila, reloj, RAM y ROM):

```python
from pypic.device import PIC16F887        # 1. declaración inicial: obligatoria para compilar (pypic build)
from pypic.gpio import RB0, RD0

CLOCK_HZ: Final[u32] = 4_000_000          # 2. frecuencia del reloj del sistema (opcional)


@init_device                               # 3. se ejecuta una vez, antes de main
def configurar() -> None:
    RD0.output()
    RB0.input()


def main() -> None:
    ...
```

- **`from pypic.device import <DISPOSITIVO>`**: debe ser la **primera línea** y única. `pypic` busca el Device Pack con ese nombre (`devices/pic16f887`) en
  `--devices-dir`, `$PYPIC_DEVICES` y `./devices`; si no existe, el error lista los disponibles. `--device <dir>` fuerza un pack concreto y debe coincidir con la declaración.
  `pypic check` y `pypic ir` funcionan sin declaración (análisis genérico, sin validar pines ni límites del dispositivo); `pypic build` la exige.
- **`CLOCK_HZ: Final[u32] = <hercios>`**: constante con ese nombre y forma exactos; se valida contra el reloj máximo del dispositivo. No ocupa RAM y no da el aviso de tipo emulado. Si el programa usa esperas (`pypic.time`, ver `docs/sdk-time.md`) y no la declara, se usa el reloj por defecto del dispositivo con un aviso.
- **`@init_device`**: una sola función `() -> None` (el nombre es libre); el arranque la llama **después de inicializar las variables y antes de `main`**.
  El código no puede llamarla. Cuenta en el análisis de pila y en la regla de no compartir funciones con la interrupción.
- Las constantes `Final` se sustituyen por su valor al compilar: **no ocupan RAM** aunque sean de 32 bits.

## Diagnosticos

Formato: `archivo:linea:col: error[PYPnnnn]: mensaje`. Rangos: `PYP0xxx` lexer, `PYP1xxx` sintaxis/subconjunto,
`PYP2xxx` tipos y semantica, `PYP3xxx` advertencias (no impiden compilar). Declaración/arranque: `PYP2090` dispositivo declarado distinto del compilado, `PYP2091` declarado dos veces, `PYP2092` no es la primera línea o hay más de un nombre, `PYP2093` `CLOCK_HZ` mal formado, `PYP2094` reloj mayor que el máximo del dispositivo, `PYP2096` nombre que no parece un PIC, `PYP2097`/`PYP2099`/`PYP2098` reglas de `@init_device`. Si hay errores de sintaxis no se ejecuta el verificador de tipos.

## Uso

```
cargo run -p pypic-cli -- check examples/counter.py [--native-bits 8] [--max-int-bits 16]
cargo test
```

## Criterios de aceptacion

- [x] Parser con errores con ubicacion y recuperacion por sentencia (`syntax_errors_report_all_statements`).
- [x] Tipado y constantes (literales, casts, overflow, `//` y `%`).
- [x] Reglas hardware-conscientes: sin recursion, una ISR, tipos limitados por el destino.
- [x] Cada construccion rechazada tiene test con su codigo.
- [ ] Pendiente: resolucion de modulos del SDK (`machine`, `gpio`...), arreglos estaticos, IR tipada,
      validacion contra Device Pack, advertencias (codigo inalcanzable, variables sin usar).
