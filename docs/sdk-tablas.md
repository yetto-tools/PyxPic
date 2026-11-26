# Tablas constantes en la memoria de programa (`Rom[uint8]`)

**Estado: implementado (#44).** El equivalente de las tablas de búsqueda del ensamblador (`ADDWF PCL,F` + `RETLW`) y de `DT`: datos que no gastan RAM, con una lectura de **costo fijo**
y el manejo correcto de `PCLATH` y de los límites de 256 palabras.

```python
SINE: Rom[uint8] = [128, 176, 218, 245, 255, 245, 218, 176]    # 8 RETLW en flash

level: uint8 = SINE[step]        # step: uint8 en ejecución
first: uint8 = SINE[0]           # índice constante: se resuelve al compilar, sin consulta
n: uint8 = len(SINE)             # 8, constante
```

## Reglas

- Se declara **a nivel de módulo** como `NOMBRE: Rom[uint8] = [...]`. `Rom` es memoria de solo lectura (la de programa, no RAM), así que **no lleva `Const`** (se acepta `Const[Rom[uint8]]`); dentro de una función es el error `PYP2160`.
- El nombre es `Rom` y solo ese: no hay sinónimos (`Table`, `Flash`...).
- De **1 a 256** entradas, cada una una constante `uint8`. El índice es un `uint8`.
- `T[i]` con índice constante se sustituye por el valor; un índice constante fuera de la tabla es error de compilación (`PYP2161`).
- `len(T)` es una constante (un literal que se adapta al tipo que lo recibe).
- Un índice que no es `uint8` es el error `PYP2163` (convierte con `uint8(...)`); indexar otra cosa, `PYP2162`; usar la tabla como valor, `PYP2164`.
- Un **índice en ejecución mayor que el último devuelve la última entrada**: nunca se ejecuta código ajeno. Es una decisión de diseño; en ensamblador, un `ADDWF PCL,F` desbordado salta a
  lo que haya a continuación.

## Costo

Una lectura con índice variable cuesta **17 ciclos con cualquier índice** (13 en una tabla de 256 entradas, que no necesita comprobación) y 14 palabras de programa más las entradas de la tabla
(una palabra cada una, compartidas entre todas sus lecturas). Medido en el simulador: el mismo número de ciclos con los índices 0, 1, 4, 8, 9, 100 y 255.

```
MOVF   idx,W                      ; 1
SUBLW  n-1                        ; 1  ─┐ satura un indice >= n a n-1:
BTFSS  STATUS,C                   ; 1  ─┤ 4 ciclos con cualquier indice
CLRW                              ; 0/1 ┤ (se omite con 256 entradas)
SUBLW  n-1                        ; 1  ─┘
MOVWF  dst                        ; 1   dst hace de temporal: no se comparte RAM, es segura en la interrupcion
MOVLW  HIGH(tabla+1) / MOVWF PCLATH ; 2   ADDWF PCL,F toma PC<12:8> de PCLATH
MOVF   dst,W / CALL tabla         ; 3
                                  ;     tabla: ADDWF PCL,F (2) · RETLW kN (2)
MOVWF  dst / CLRF PCLATH          ; 2   se restablece la invariante de la ABI (PCLATH = pagina 0)
```

Cada lectura usa **un nivel de la pila de hardware** (el `CALL`), y el análisis de profundidad lo cuenta (`PYP2061`). Se puede leer una tabla desde una `@interrupt`: la rutina no usa RAM compartida y la entrada de
la ISR guarda y restablece `PCLATH`.

## Colocación: el límite de 256 palabras

`ADDWF PCL,F` solo suma al byte bajo del contador de programa: si la tabla cruzara un límite de 256 palabras, el acarreo no pasaría al byte alto y las entradas posteriores serían otras.
El enlazador **mueve la tabla** (dejando huecos sin programar) para que sus `n` entradas caigan en un solo bloque de 256; una tabla de 256 entradas queda alineada a 256. La prueba barre el tamaño del código
anterior para que la tabla caiga a ambos lados de `0x100` y comprueba que ninguna cruza y que todas dan el resultado correcto.

## Límites

- Solo `uint8` (cada entrada es un `RETLW`); tablas de `uint16` y de cadenas, más adelante.
- Con un programa de más de una página (docs/abi-pic14.md, «Programas de más de una página») la consulta cuesta **18 ciclos** (restablece los bits de página de PCLATH en lugar de limpiarlo).
- La tabla se declara una vez a nivel de módulo; no hay arreglos en RAM (`buf: uint8[16]` sigue siendo #8).

## Verificación

`crates/pypic-pic14/tests/tables.rs`: prueba diferencial intérprete de la IR ↔ simulador del núcleo (suma y XOR de la tabla, índice fuera de rango, tabla de 256 entradas, varias tablas
dentro de funciones con parámetros), costo idéntico con siete índices, barrido del límite de 256 y todas las reglas del lenguaje. Ejemplo: `examples/python/tablas/table.py`.
