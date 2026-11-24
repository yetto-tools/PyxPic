# Operadores con rutina nativa: `*`, `//`, `%` y desplazamientos variables

**Estado: implementado (#27).** El PIC16 no tiene instrucciones de multiplicar, dividir ni desplazar por una cantidad variable. PyPIC las ofrece como **operadores** (`a * b`, `a // b`, `a % b`, `x << n`, `x >> n`)
que se compilan a **rutinas nativas estáticas** (nunca un intérprete), con el costo a la vista:

- Un aviso **`PYP3011`** por rutina y programa dice cuál se usa (`mul8`, `divmod16`...), cuántos niveles de pila gasta y remite a esta página.
- `--listing` muestra la rutina (`rt.mul8:`) y sus instrucciones.
- Los costos de abajo los **mide el simulador** en las pruebas (`crates/pypic-pic14/tests/runtime.rs`): si cambian, la prueba falla.

## Qué rutina usa cada operación

| Operación | Código | Costo |
|---|---|---|
| `a * K`, `K * a` con `K` constante 0 o `2^k` | desplazamiento o borrado, **sin rutina** | `k` pasos de 1 + `n` ciclos |
| `a // K`, `a % K` **sin signo** con `K = 2^k` | `k` desplazamientos a la derecha / una máscara, **sin rutina** | igual |
| `a * b` (8 bits) | `mul8` | **116 ciclos**, siempre; 16 palabras |
| `a * b` (16 bits) | `mul16` | **339 ciclos**, siempre; 24 palabras |
| `a // b`, `a % b` sin signo, 8 bits | `divmod8` | 148–172 ciclos; 23 palabras |
| `a // b`, `a % b` sin signo, 16 bits | `divmod16` | 371–515 ciclos; 35 palabras |
| `a // b`, `a % b` con signo, 8 bits | `sdivmod8` (llama a `divmod8`) | 17 (divisor 0) – 195 ciclos; 31 palabras |
| `a // b`, `a % b` con signo, 16 bits | `sdivmod16` (llama a `divmod16`) | 28 (divisor 0) – 543 ciclos; 48 palabras |
| `x << n`, `x >> n` con `n` variable (8 bits) | bucle **en línea**, sin `CALL` | 13 + 8 por bit; con `n ≥ 8`: 78 |
| `x << n`, `x >> n` con `n` variable (16 bits) | bucle en línea | 19 + 9 por bit; con `n ≥ 16`: 164 |

La multiplicación es **de tiempo constante** (útil dentro de una espera o una interrupción medida). La división recorre `8·n` pasos y su tiempo depende de los datos dentro del rango dado.
Los ciclos de las rutinas incluyen la carga de operandos y la lectura del resultado (las medidas son de `r = x OP y` entre dos escrituras de `RD0`).

## Semántica

- **Aritmética modular** en el ancho del tipo, igual que la suma: `uint8: 200 * 2 = 144`. La multiplicación es la misma con y sin signo.
- **División y módulo con signo**: truncan hacia cero y el resto lleva el signo del dividendo (como C): `-7 // 2 = -3`, `-7 % 2 = -1`; `-128 // -1` envuelve a `-128`.
- **Divisor cero en ejecución** (un divisor *constante* cero es un error de compilación): no hay trampa en un PIC, así que el resultado está **definido**: `a // 0` es **todo unos** del ancho del tipo
  (`255`, `65535`, y `-1` con signo) y `a % 0` es **`a`**. Es lo que sale de dividir por restas sucesivas, y el intérprete de referencia hace lo mismo.
- **Desplazamientos variables**: la cantidad se acota al ancho del tipo. `x << n` y `x >> n` sin signo dan `0` si `n ≥ 8·bytes`; `>>` con signo rellena con el signo (`-2 >> 200 = -1`).
  Una cantidad de 16 bits con byte alto distinto de cero es una cantidad enorme.
- Solo 8 y 16 bits. `uint32`/`int32` son #32.

## Una copia por contexto

La memoria de trabajo de las rutinas es estática, así que si la interrupción entrara en mitad de una multiplicación de `main` que usa la misma rutina, la dañaría. Por eso cada contexto
(**main** y **la interrupción**) que use alguna rutina tiene **su propia copia** (`rt.mul16` y `rt.mul16.isr`) y su propio bloque de **9 bytes de RAM** (`A`, `B`, `R`, contador, máscara, signo). Solo se generan las
que se piden y solo en los contextos que las usan. Una prueba inyecta una interrupción en cada instante de la multiplicación principal y comprueba que el resultado no cambia.

## Pila de hardware

Cada rutina es un `CALL`: **un nivel** (la división con signo, **dos**, porque llama a la sin signo). El análisis de profundidad del frontend lo cuenta (`PYP2061`), igual que las consultas de tablas `Rom`.
Los desplazamientos variables y las operaciones por potencia de 2 no usan pila.

## Verificación

`crates/pypic-pic14/tests/runtime.rs` (todo contra el intérprete de la IR): **todos los pares** de `uint8` e `int8` (65 536) para `*`, `//` y `%` incluido el divisor 0; 24 valores de borde de 16 bits (576 pares) con y sin signo;
todos los desplazamientos de 0 a 39 en `uint8`, `int8`, `uint16` e `int16`; resultados conocidos; potencias de 2 sin rutina; una copia por contexto con interrupciones inyectadas; pila; y los costos de esta tabla.
