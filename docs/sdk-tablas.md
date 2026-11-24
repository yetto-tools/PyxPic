# Tablas constantes en la memoria de programa

**Estado: implementado (#44).** El equivalente de las tablas de búsqueda del ensamblador (`ADDWF PCL,F` + `RETLW`) y de `DT`: datos que no gastan RAM, con una lectura de **costo fijo**
y el manejo correcto de `PCLATH` y de los límites de 256 palabras.

```python
SINE: Final[Table[u8]] = [128, 176, 218, 245, 255, 245, 218, 176]    # 8 RETLW en flash

level: u8 = SINE[step]        # step: u8 en ejecución
first: u8 = SINE[0]           # índice constante: se resuelve al compilar, sin consulta
n: u8 = len(SINE)             # 8, constante
```

## Reglas

- Se declara **a nivel de módulo** como `NOMBRE: Final[Table[u8]] = [...]` (una tabla es de solo lectura; sin `Final` o dentro de una función es el error `PYP2160`).
- De **1 a 256** entradas, cada una una constante `u8`. El índice es un `u8`.
- `T[i]` con índice constante se sustituye por el valor; un índice constante fuera de la tabla es error de compilación (`PYP2161`).
- `len(T)` es una constante (un literal que se adapta al tipo que lo recibe).
- Un índice que no es `u8` es el error `PYP2163` (convierte con `u8(...)`); indexar otra cosa, `PYP2162`; usar la tabla como valor, `PYP2164`.
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

- Solo `u8` (cada entrada es un `RETLW`); tablas de `u16` y de cadenas, más adelante.
- El programa entero cabe en la página 0 (2048 palabras; `PCLATH` para programas mayores es #31).
- La tabla se declara una vez a nivel de módulo; no hay arreglos en RAM (`buf: u8[16]` sigue siendo #8).

## Verificación

`crates/pypic-pic14/tests/tables.rs`: prueba diferencial intérprete de la IR ↔ simulador del núcleo (suma y XOR de la tabla, índice fuera de rango, tabla de 256 entradas, varias tablas
dentro de funciones con parámetros), costo idéntico con siete índices, barrido del límite de 256 y todas las reglas del lenguaje. Ejemplo: `examples/table.py`.
