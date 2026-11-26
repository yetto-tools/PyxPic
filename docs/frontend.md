# Frontend del compilador (`pypic-frontend`)

## Nombres de tipos enteros y migración

Los nombres recomendados son `uint8`, `int8`, `uint16`, `int16`, `uint32` e `int32`: `uint` indica sin signo, `int` indica con signo y el número especifica los bits. Sus rangos y representación no cambian: los tipos de 8, 16 y 32 bits almacenan respectivamente 1, 2 y 4 bytes de datos; el código y los temporales pueden necesitar recursos adicionales.

Para migrar, sustituye `u8` por `uint8`, `i8` por `int8`, `u16` por `uint16`, `i16` por `int16`, `u32` por `uint32` e `i32` por `int32`, también en conversiones y declaraciones `Const` y `Rom`. Los nombres anteriores siguen aceptándose por compatibilidad; no se ha fijado una fecha de eliminación ni se emite aún un aviso de obsolescencia. Las capacidades de cada backend no cambian por renombrar los tipos.

**Estado: implementado (primer corte).** Parseo + validacion del subconjunto + tipos + constantes.
No genera IR ni codigo todavia. Escrito en Rust; Python es solo la sintaxis del lenguaje fuente.
Todo corre en el host: no hay interprete ni VM de Python en el MCU
(ver `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).

```
fuente .py -> lexer -> parser (AST propio) -> check (tipos, constantes, reglas) -> Program tipado
```

## Independencia del ancho del MCU

PyPIC no es "solo 8 bits": cubre cada MCU PIC. El lenguaje tiene enteros de ancho fijo
(`uint8 int8 uint16 int16 uint32 int32`) y `bool`; **no hay `int` nativo ni ancho de palabra asumido**.
Que tipos admite un destino lo decide `TargetProfile` (mas adelante lo llenara el Device Pack):

- `native_bits`: ancho nativo de la CPU (8 en PIC16, 16 en PIC24/dsPIC, 32 en PIC32...).
- `max_int_bits`: ancho maximo admitido, nativo o emulado.

**Decision #1:** un tipo mas ancho que `native_bits` pero dentro de `max_int_bits` **se permite** y se emula con rutinas
nativas de software; el compilador emite una advertencia de coste (`PYP3001`, una vez por tipo). Por encima de
`max_int_bits` es error (`PYP2030`). Sin perfil, el destino es `generic` (32 bits nativos).

## Subconjunto soportado

- Nivel de modulo: `x: uint8 = 0`, `K: Const[uint8] = 5` (constante), funciones, `from pypic import ...`.
- Funciones: parametros y retorno tipados (`-> None` si no devuelve), `@interrupt` (una sola, sin parametros).
- Sentencias: asignacion, `+= -= *= //= %= &= |= ^= <<= >>=`, `if/elif/else`, `while`,
  `for i in range(...)`, `return`, `break`, `continue`, `pass`, `global`, llamadas.
- Expresiones: enteros (dec/hex/bin/octal), `True/False`, aritmetica, bit a bit, desplazamientos,
  comparaciones, `and/or/not`, llamadas, conversiones `uint8(x)`.
- `uint8 int8 uint16 int16 uint32 int32 bool Const interrupt` estan predeclarados; `from pypic import ...` es opcional
  (mantiene el archivo valido para otros editores).

Todo lo demas se rechaza con un diagnostico especifico: clases, listas/tuplas, cadenas, floats, `/`, `**`,
lambdas, `try`, `import` general, atributos, indexado, argumentos con nombre, comparaciones encadenadas, etc.

## Semantica fijada (confirmada, decision #2)

| Tema | Regla |
|---|---|
| Literales | Sin tipo propio; toman el del contexto si su valor cabe (`PYP2040` si no). |
| Conversiones | Siempre explicitas: `uint16(a) + b`. Mezclar tipos es `PYP2047`. |
| Overflow en ejecucion | Modular (wrap-around) en el ancho del tipo. |
| Overflow constante | Error (`PYP2045`/`PYP2040`), nunca truncado en silencio. |
| Cast explicito | Trunca a proposito: `uint8(uint16(300)) == 44`. |
| `//` y `%` | Truncan hacia cero (como C), **no** como Python. `/` no existe. |
| Condiciones | Deben ser `bool` (sin truthiness): `if x != 0`. |
| Variables | Siempre inicializadas al declararse; las globales con constante. Declarar con tipo antes de asignar. |
| Globales | Modificarlas en una funcion exige `global x`. Un local no puede ocultar una global. |
| `for` | Solo `range(...)`; el paso debe ser constante distinto de 0; si todo es literal, el tipo es el menor que cabe. |
| Recursion | Prohibida (memoria estatica, pila de hardware limitada): `PYP2013`. |
| ISR vs main | **Decision #3.** Una funcion alcanzable desde `main` y desde la `@interrupt` es error (`PYP2060`): su memoria estatica seria compartida y la ISR podria corromperla. Se escribe una version por contexto. El contexto principal es `main`; si no existe, toda funcion sin llamadores que no sea la ISR. Compartir *globales* si esta permitido. |
| Pila de retorno | Si el destino declara `stack_levels` (8 en PIC16), el peor caso `1 + prof(main) + 1 + prof(ISR)` no puede exceder el limite: `PYP2061` (ver `docs/abi-pic14.md`). |
| Ensanchamiento | Sin conversion implicita ni siquiera sin perdida (`uint8` + `uint16` es `PYP2047`). |
| Desplazamientos | Cantidad literal `0..bits-1` o entero sin signo; el operando izquierdo necesita tipo. |

## Declaración del dispositivo, reloj y arranque

Un programa dice **a qué PIC está orientado** en su primera línea, y de ahí salen todas las validaciones (pines, tipos, pila, reloj, RAM y ROM):

```python
from pypic.device import PIC16F887        # 1. declaración inicial: obligatoria para compilar (pypic build)
from pypic.periph.gpio import RB0, RD0

CLOCK: Internal[4 * MHz]                     # 2. fuente y frecuencia del reloj (opcional; docs/sdk-reloj.md)


@init_device                               # 3. se ejecuta una vez, antes de main
def configurar() -> None:
    RD0.output()
    RB0.input()


def main() -> None:
    ...
```

- **`from pypic.device import <DISPOSITIVO>`**: debe ser la **primera línea** y única. `pypic` busca el Device Pack con ese nombre (`devices/pic16f887`) en
  `--devices-dir`, `$PYPIC_DEVICES` y `./devices`; si no existe, el error lista los disponibles. `--device <dir>` fuerza un pack concreto y debe coincidir con la declaración.
  `pypic check` y `pypic ir` funcionan sin declaración (análisis genérico, sin validar pines ni límites del dispositivo), pero **lo avisan** (`PYP3017`); `pypic build` la exige.
  **Va siempre en el código**, también en un proyecto con `pypic.toml`: el mismo programa puede ser válido para varios PIC y la declaración es lo que dice **para cuál está escrito** y,
  por tanto, qué se espera tener: de ella salen los pines (`PYP2150`), los periféricos (`from pypic.periph.pwm import PWM1` es un error si el dispositivo no trae CCP1: `PYP2200`, `PYP2210`...),
  los registros, los tipos nativos, la pila, el reloj máximo, la RAM y la ROM. Si el dispositivo viene solo del perfil de `pypic.toml` y el código no lo declara, se avisa (`PYP3018`).
- **`CLOCK: Internal[4 * MHz]`** (o `Crystal[20 * MHz]`, `ExternalClock[20 * MHz]`): la forma recomendada de declarar el reloj, ver `docs/sdk-reloj.md`. Elige la fuente y la frecuencia, configura `FOSC` y `OSCCON` y se valida contra el reloj máximo del dispositivo. No ocupa RAM y no da el aviso de tipo emulado. Si el programa usa esperas (`pypic.system.time`, ver `docs/sdk-time.md`) y no declara el reloj, se usa el reloj por defecto del dispositivo con un aviso.
- **`CLOCK_HZ: Const[uint32] = <hercios>`**: la forma manual, con ese nombre y forma exactos: solo declara la frecuencia (el oscilador lo configuras tú); se valida contra el reloj máximo. No se puede combinar con `CLOCK`.

**Implementado (#69, `docs/sdk-reloj.md`):** `CLOCK: Internal[4 * MHz]`, `CLOCK: Crystal[20 * MHz]` y `CLOCK: ExternalClock[20 * MHz]`, sin imports (el formato `Nombre[frecuencia]` es Python válido; se descartaron `CLOCK = "4MHz internal"` y el constructor con paréntesis). La expresión se resuelve en compilación, configura `FOSC` y `OSCCON` y valida contra el datasheet. `CLOCK_HZ` sigue admitiéndose como declaración de frecuencia compatible, sin configuración automática del oscilador. La frecuencia declarada es FOSC; en PIC16F887 el reloj de instrucción es FOSC/4.
- **`@init_device`**: una sola función `() -> None` (el nombre es libre); el arranque la llama **después de inicializar las variables y antes de `main`**.
  El código no puede llamarla. Cuenta en el análisis de pila y en la regla de no compartir funciones con la interrupción.
- Las constantes `Const` se sustituyen por su valor al compilar: **no ocupan RAM** aunque sean de 32 bits.

## Diagnosticos

Formato: `archivo:linea:col: error[PYPnnnn]: mensaje`. Rangos: `PYP0xxx` lexer, `PYP1xxx` sintaxis/subconjunto,
`PYP2xxx` tipos y semantica, `PYP3xxx` advertencias (no impiden compilar). Declaración/arranque: `PYP2090` dispositivo declarado distinto del compilado, `PYP2091` declarado dos veces, `PYP2092` no es la primera línea o hay más de un nombre, `PYP2093` `CLOCK_HZ` mal formado, `PYP2094` reloj mayor que el máximo del dispositivo, `PYP2096` nombre que no parece un PIC, `PYP2097`/`PYP2099`/`PYP2098` reglas de `@init_device`. Si hay errores de sintaxis no se ejecuta el verificador de tipos.

## Uso

```
cargo run -p pypic-cli -- check examples/python/gpio/counter.py [--native-bits 8] [--max-int-bits 16]
cargo test
```

## Criterios de aceptacion

- [x] Parser con errores con ubicacion y recuperacion por sentencia (`syntax_errors_report_all_statements`).
- [x] Advertencias reales (#5, `crates/pypic-frontend/tests/warnings.rs`; no impiden compilar y `pypic check` las imprime y sale con 0): **`PYP3020`** codigo inalcanzable tras `return`, `break`, `continue`, un `if` cuyas dos ramas terminan o un `while True` sin `break` (una vez por bloque, en la primera sentencia inalcanzable; un `pass` suelto no cuenta; el codigo inalcanzable se sigue comprobando); **`PYP3021`** variable local declarada que nunca se lee (asignarla de nuevo no cuenta como leerla; `x += 1` si cuenta, para no avisar de un contador); **`PYP3022`** parametro que nunca se usa (no se comprueba en `@asm_function`, que usa los parametros desde el ensamblador). Un nombre que empieza por `_` y la variable de un `for` nunca avisan.
- [x] Errores de sintaxis y de tipos a la vez (#4): tras un error de sintaxis se siguen comprobando los tipos de lo que el parser si leyo (la recuperacion descarta la linea rota y su bloque). Solo se suman **errores** (no avisos) y se descartan los que dependen de lo descartado (`PYP2043` nombre o funcion no definidos, `PYP2022`, `PYP2020`, `PYP2023`, `PYP2171`, `PYP2181`): un nombre cuya declaracion era la linea rota no se reporta como indefinido. No hay programa mientras haya errores de sintaxis. `crates/pypic-frontend/tests/recovery.rs`, incluida una prueba que rompe cada linea de todos los ejemplos (mas de 2000 programas) para asegurar que el verificador no se cae con un AST incompleto.
- [x] Tipado y constantes (literales, casts, overflow, `//` y `%`).
- [x] Reglas hardware-conscientes: sin recursion, una ISR, tipos limitados por el destino.
- [x] Cada construccion rechazada tiene test con su codigo.
- [ ] Pendiente: resolucion de modulos del SDK (`machine`, `gpio`...), IR tipada, (arreglos estaticos `uint8[N]`: hechos, ver `sdk-arreglos.md`),
      validacion contra Device Pack, advertencias (codigo inalcanzable, variables sin usar).
