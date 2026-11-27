# Optimizaciones de la IR

**Estado: implementado (#6).** El compilador optimiza la IR antes de generar código (`pyxpic_ir::lower`). Son pasadas **conservadoras, por función y sin perder semántica**: el programa optimizado se comporta exactamente igual que el original. `pyxpic build --no-opt` (y `pyxpic ir --no-opt`) las desactiva, para depurar o comparar tamaños; `pyxpic_ir::lower_unoptimized` es la IR tal como sale del frontend.

## Qué hace cada pasada

| Pasada | Qué hace | Qué **no** toca |
|---|---|---|
| `propagate` | Dentro de cada bloque, propaga **constantes y copias** de temporales y locales a sus lecturas y **pliega** las operaciones que quedan con operandos constantes (`Binary`, `Unary`, `Compare`, `Cast`) | Los **globales** (una interrupción puede cambiarlos entre dos lecturas) y los intrínsecos de `pyxpic.bits` (`bit_set` y `bit_clear` modifican su variable en su sitio) |
| `simplify_cfg` | Una rama con condición constante o con los dos destinos iguales pasa a salto; salta los bloques vacíos que solo saltan a otro (`bb2: jump bb3`); funde un bloque con su único sucesor; quita los bloques inalcanzables | El bloque de entrada sigue siendo el 0 (aunque esté vacío) |
| `eliminate_dead` | Quita las instrucciones **puras** cuyo resultado (temporal o local) nunca se lee: `Copy`, `Unary`, `Binary`, `Compare`, `Cast`, `RomGet` | Llamadas, esperas, pines, periféricos, escrituras a globales y arreglos, y las lecturas de arreglos (el intérprete anota los accesos fuera de rango) |
| `compact_temps` | Renumera los temporales que quedan, para que los muertos no gasten RAM | — |

Las pasadas se repiten hasta que ninguna cambia nada (como mucho 16 veces). Las funciones `@asm_function` **no se tocan**.

**La misma semántica que el intérprete:** el plegado de constantes usa las mismas funciones que el intérprete de referencia (`pyxpic_ir::eval`: suma modular, división por cero definida, desplazamientos, comparaciones con signo), así que no pueden discrepar.

## Qué **no** hace

- No propaga **entre bloques**, no elimina subexpresiones comunes, no reordena instrucciones, no hace álgebra (`x + 0`, `x * 1`), no hace desenrollado ni inlining.
- No toca los **globales** ni los accesos a registros, pines y periféricos: son efectos observables.
- No reduce los **locales** declarados (ocupan RAM aunque nunca se lean después de optimizar; `PYP3021` avisa de las variables que nadie lee).

## Cuánto ahorra (medido)

Es **modesto**: el frontend y el backend ya generan código ajustado. Sobre los 25 ejemplos del repositorio, con `pyxpic build` y con `--no-opt`:

| | Sin optimizar | Optimizado |
|---|---|---|
| Flash (suma de los 25) | 5115 palabras | **5010** (−105, ≈ 2 %) |
| Mayores ahorros | `convertidor.py` 1286 → 1272 · `gps_nmea.py` 329 → 315 · `counter_serial.py` 1261 → 1247 · `direccion_9bits.py` 151 → 141 · `saludo.py` 109 → 99 | |
| RAM | −1 byte en `counter.py`, `media_movil.py`, `counter_serial.py` y `saludo.py` | |
| Tiempo | `parse_dec`/`parse_hex` ≈ 10 % menos ciclos (por ejemplo `parse_dec("65535")`: 564 → 549) | |

Muchos ejemplos no cambian (`blink.py`, `adc.py`...): ya eran mínimos. Las cifras de costo documentadas de otros módulos se midieron en el simulador y siguen siendo las mismas.

## Cómo se comprueba

- **`crates/pyxpic-ir/tests/opt.rs`:** lo que hace cada pasada antes y después (constantes plegadas, rama eliminada, bloques fundidos, temporales renumerados); lo que **no** debe tocar (efectos secundarios, globales —un valor guardado antes de cambiar el global no se sustituye—, `bits` en sitio, funciones `asm`); que optimizar dos veces no cambia nada más; y que **`verify()` pasa tras cada pasada**, con cada programa de la prueba y con todos los ejemplos del repositorio.
- **El programa optimizado se comporta igual:** ocho programas (aritmética de 8 y 16 bits con signo, bucles con `break` y `continue`, arreglos con índices fuera de rango, llamadas, la UART con `read_line`, `parse_dec` y f-strings, y variables leídas entre cambios) dan los mismos globales, los mismos bytes por la UART y los mismos arreglos con la IR original y con la optimizada.
- **El oráculo del backend es la IR sin optimizar:** las pruebas diferenciales de `crates/pyxpic-pic14/tests/codegen.rs` (programas aleatorios) comparan ahora el simulador del PIC, con la IR **optimizada**, contra el intérprete sobre la IR **sin optimizar**. Así comprueban el optimizador y el backend a la vez.
- Las pruebas de forma de la IR (`crates/pyxpic-ir/tests/ir.rs`) miran lo que genera el frontend: usan la IR sin optimizar.

**Solo probado en simulador.** Nada de esto se ha probado en hardware.
