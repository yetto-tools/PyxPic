# Arreglos estáticos de RAM (`uint8[N]`)

**Estado: implementado (primera entrega de #8).** Arreglos de capacidad fija y elementos `uint8` en la RAM del banco 0, declarados a nivel de módulo. El diseño y lo que queda fuera están en `docs/diseno-arreglos.md`.

```python
muestras: uint8[8]              # 8 bytes de RAM, a cero al arrancar

def main() -> None:
    muestras[0] = 200           # índice constante: acceso directo
    i: uint8 = 3
    muestras[i] = muestras[0]   # índice variable: FSR / INDF
    muestras[i] += 1            # buf[i] op= v: se lee, se opera y se escribe con el mismo índice
    n: uint8 = len(muestras)    # la capacidad (8), constante
```

## Reglas

- **Solo globales y solo `uint8`.** Un arreglo local, de otro tipo (`uint16[4]`, `int8[4]`) o con capacidad fuera de 1 a 255 (`PYP2171`) es un error. Sin inicializador, los elementos empiezan **en cero**: el arranque pone a cero toda la RAM de cada arreglo (hasta 4 elementos con `CLRF`; más, con un bucle sobre `FSR`).
- **Inicializador:** `tabla: uint8[4] = [10, 20, 30, 40]` da **todos** los valores (la lista trae exactamente la capacidad; `PYP2160` si faltan, sobran, no son constantes o no caben en un `uint8`). El arranque los escribe en la RAM antes de `main`: `MOVLW` + `MOVWF` por cada valor distinto de cero (2 palabras de flash) y `CLRF` por cada cero (1 palabra), sin bucle, aunque sean más de 4. Con hasta 4 elementos eso es **una palabra más por cada valor distinto de cero** que sin inicializador; con más de 4, el arreglo sin inicializador se pone a cero con un bucle corto y el inicializado ocupa de 1 a 2 palabras por elemento. Para empezar en cero se omite el `= ...`. No es una tabla `Rom`: vive en RAM y se puede escribir.
- **No hace falta `global`** para escribir un elemento (no se reasigna el nombre), igual que en Python.
- **El nombre solo no es un valor:** `x = buf` y `buf = 3` son `PYP2172`; se usa `buf[i]` y `len(buf)`.
- **Índice:** un `uint8` (o una constante). Una constante fuera de la capacidad es el error `PYP2161`; otro tipo de índice, `PYP2163` (convierte con `uint8(...)`).
- **Los arreglos `Rom[uint8]` no cambian:** siguen siendo de solo lectura, en la memoria de programa (`PYP2162` si se escribe en una).
- **No se pasan ni se devuelven** de una función; las funciones los usan por su nombre global.

## Índice variable fuera de rango: se satura, nunca escribe fuera

Un índice variable **mayor o igual que la capacidad se satura a la última posición** (`buf[9]` en un `uint8[4]` es `buf[3]`), tanto al leer como al escribir; el comportamiento está definido y **ninguna escritura llega a otra variable ni a un registro**. Es la misma regla que las tablas `Rom`.

Es una elección de PyxPic, no la única posible: no avisa en el PIC (no hay forma barata de hacerlo) y puede **ocultar un error del programa**. Por eso el **intérprete de referencia anota cada acceso fuera de rango** (`Machine::array_oob`), para que una prueba lo vea; el simulador del PIC no.

## Costo (fijo, medido en el simulador)

| Operación | Ciclos |
|---|---|
| `v = buf[3]`, `buf[3] = 7` (índice constante) | 2 |
| `v = buf[i]`, `buf[i] = 7` (índice variable) | 9 |
| `buf[i] += 1` (índice variable) | 21 (leer 9 + operar + escribir 9) |

El costo del índice variable **no depende del valor** (dentro o fuera de rango). El recorte del índice es:

```
MOVF index,W / SUBLW len-1 / BTFSS STATUS,C / CLRW / SUBLW len-1   ; satura un índice >= len a len-1
ADDLW base / MOVWF FSR  ->  MOVF INDF,W  (o  MOVWF INDF )
```

Requiere `IRP` = 0 y el arreglo en el banco 0: la ABI mantiene `IRP` en 0 y todo el dato del compilador está en el banco 0.

## RAM y límites

El backend usa **solo el banco 0**: `20h–6Fh` (80 bytes) compartidos entre globales, arreglos, locales y temporales. Un arreglo se reserva **explícitamente** y, si no cabe, la compilación falla con `OutOfRam` (no se recorta ni se pasa a otro banco). No hay overlay de memoria todavía (#26): cada arreglo ocupa su RAM durante todo el programa.

## Interrupciones

El acceso indirecto usa `FSR`, que **una interrupción podría pisar** a mitad de un acceso de `main`. Si la interrupción (o algo que llama) usa arreglos, **su prólogo guarda `FSR` en `73h` y su epílogo lo restaura**; si no los usa, no se gasta nada. Una prueba inyecta una interrupción **en cada instrucción** de un programa con accesos indirectos y comprueba que el resultado de `main` no cambia. Los datos del arreglo que se comparten entre `main` y la interrupción son de las dos: protegerlos de accesos no atómicos es del programa, igual que con cualquier global.

## Diagnósticos nuevos

| Código | Regla |
|---|---|
| `PYP2170` | Arreglo local, de tipo que no es `uint8`, o con inicializador |
| `PYP2171` | Capacidad fuera de 1 a 255 |
| `PYP2172` | Un arreglo usado como valor o asignado entero |
| `PYP2161` | Índice constante fuera de la capacidad |
| `PYP2162` | Se indexa algo que no es tabla ni arreglo, o se escribe en una tabla `Rom` |
| `PYP2163` | Índice que no es `uint8` |

## Lo que no está

`uint16` y otros tipos de elemento, arreglos locales, inicializadores, `char[N]` y cadenas en RAM, recepción de líneas y `format_into` (de #63), pasar arreglos a funciones, arreglos en otros bancos y su solapamiento por grafo de llamadas (#26).

## Verificación

`crates/pyxpic-pic14/tests/arrays.rs`: PIC14 contra el intérprete de la IR en lectura, escritura, asignación compuesta y `len`; dos arreglos sin solaparse; **la RAM completa idéntica** entre un índice saturado y uno en rango; el arranque en cero con la RAM llena de `FF`; la interrupción en cada instrucción; los diagnósticos; RAM agotada; y los costos de arriba. Ejemplo: `examples/python/adc/media_movil.py` (media móvil de 8 lecturas del ADC, probada en simulador con tres entradas). **No probado en hardware.**
