# PyxPic BASIC (estilo PICAXE, `.bas`)

**Estado: primera entrega.** Es el segundo lenguaje de PyxPic. Los tres caminos para escribir un programa de PIC quedan así:

| Lenguaje | Extensión | Qué es |
|---|---|---|
| **Python** | `.py` | El lenguaje principal: sintaxis Python estática y tipada, SDK de periféricos, ensamblador en línea (`@asm_function`) |
| **PyxPic BASIC** | `.bas` | BASIC al estilo del que ofrece PICAXE (`HIGH`, `PAUSE`, `READADC`, `GOTO`, `GOSUB`, `b0`/`w0`), **compilado a código nativo** |
| **Ensamblador** | `.s` | PIC-AS de XC8 (`pyxpic asm`, docs/asm-pic14.md) |

BASIC **no tiene intérprete ni VM**: el archivo `.bas` se traduce al mismo árbol (AST) que produce el parser de Python y de ahí pasa por el mismo verificador, el mismo IR, el mismo backend PIC14 y el mismo simulador. Todo lo que Python sabe hacer con el SDK (ADC, UART, EEPROM, comparadores, PWM, temporizadores) está disponible desde BASIC con las mismas reglas, los mismos costos en ciclos y los mismos diagnósticos de tipos y de periféricos, ubicados en la línea del `.bas`.

```basic
' parpadeo.bas
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

SYMBOL led = D.0

main:
  HIGH led
  PAUSE 500
  LOW led
  PAUSE 500
  GOTO main
```

```
pyxpic check parpadeo.bas
pyxpic build parpadeo.bas -o parpadeo.hex
```

La CLI elige el lenguaje **por la extensión**: `.bas` es BASIC y cualquier otra cosa, Python. Un `pyxpic.toml` acepta `main = "main.bas"`; `pyxpic project check`, `pyxpic build` y `pyxpic clean` funcionan igual.

## Qué es igual y qué es distinto que en PICAXE

PyxPic BASIC parte del BASIC de PICAXE para que sus programas se reconozcan, pero es un lenguaje compilado y tipado, y por eso **no es una copia**:

| | PICAXE | PyxPic BASIC |
|---|---|---|
| Dispositivo | el que elige el editor (`#picaxe 40X2`) | **`DEVICE PIC16F887`**, siempre en el código |
| Pines | `B.0`, `pinB.0`, números de pin | `B.0` (salida) y `pinB.0` (entrada) son el mismo pin RB0; también `SYMBOL led = B.0` |
| Variables | `b0`..`b27`, `w0`..`w13`, `bit0`.. | `b0`..`b27` (BYTE) y `w0`..`w13` (WORD) existen sin declarar; **no se solapan** (ver abajo); `DIM` para las propias; **sin `bitN`** |
| Operadores | **de izquierda a derecha, sin precedencia** | **precedencia normal** (`*` y `/` antes que `+` y `-`); se documenta aquí porque un programa PICAXE portado puede dar otro resultado |
| Aritmética | siempre de 16 bits | **en el ancho de las variables y del destino** (ver abajo) |
| Signo | sin signo | sin signo; no hay números negativos |
| `//` y `**` | resto y parte alta de la multiplicación | no existen: el resto es `MOD` o `%` |
| Overflow al asignar | trunca en silencio | **error de compilación** si una constante no cabe; una expresión se trunca con una conversión visible |
| `END` | dormir | bucle infinito |

## El encabezado

```basic
DEVICE PIC16F887            ' obligatorio: dice para qué PIC está escrito el programa
CLOCK INTERNAL 8MHZ         ' la fuente y su frecuencia como literal con unidad (como `CLOCK: Internal` y `FREQUENCY: 8MHz` en Python)
'   CLOCK INTERNAL         -> 4 MHz (el valor de reset)         CLOCK CRYSTAL 11.0592MHZ MODE HS
'   CLOCK EXTERNAL 8MHZ    -> señal de reloj en CLKIN           CLOCK RESONATOR 8MHZ     CLOCK RC 2MHZ
'   la forma anterior `CLOCK INTERNAL 4 MHZ` (con espacio) sigue valiendo
'   ajustes opcionales, uno por línea: PLL 4 · CLOCKDIVIDER 2 · CLOCKOUTPUT ON · FAILSAFE ON · CLOCKSWITCHING ON
BAUD 9600                   ' velocidad de la UART (8N1): la de SERTXD, SERRXD y UART.write; sin esta línea son 9600
```

**No hay importaciones.** Los periféricos del SDK (`ADC`, `UART`, `EEPROM`, `PWM1`, `CMP1`, `CVREF`, `TMR2`...) y sus constantes (`AN0`, `FOSC_8`, `C12IN0`, `REF_PIN`...) se importan solos cuando el programa los usa, y no distinguen mayúsculas (`adc`, `an0`). Los pines y las pausas también. Solo los módulos que no son periféricos (`pyxpic.regs`) se importan con `FROM pyxpic.regs IMPORT STATUS`; hacerlo con un periférico sigue funcionando.

## Variables

```basic
b0 = 200                    ' b0..b27: BYTE (0 a 255), existen siempre
w3 = 1000                   ' w0..w13: WORD (0 a 65535)
DIM total AS WORD           ' variables con nombre: BYTE, BIT (hoy es un BYTE; no se restringe a 0 y 1) o WORD
DIM tabla(16) AS BYTE       ' arreglo de BYTE (el tamaño es fijo; un índice fuera de rango se satura al último)
SYMBOL limite = 100         ' constante
SYMBOL contador = b3        ' otro nombre para una variable
SYMBOL led = D.0            ' un pin
```

Todas las variables son **globales** y empiezan en 0. Los nombres no distinguen mayúsculas de minúsculas.

**`w0` y `b0`/`b1` no se solapan.** En PICAXE `w0` es `b0:b1`, `w1` es `b2:b3`, y así. El IR de PyxPic no tiene lugares que se pisen, así que BASIC los trata como variables **independientes** y, en vez de dejar que un programa dependa del solapamiento sin saberlo, **da un error (`PYB0074`) si el programa usa a la vez `wN` y `b(2N)` o `b(2N+1)`**. Usa solo una de las dos formas, o `DIM` con nombres.

### El ancho de una expresión

Una expresión se calcula en el ancho de sus variables y del destino, no siempre en 16 bits como en PICAXE (que costaría instrucciones de más en un PIC de 8 bits):

```basic
b2 = b0 + b1        ' suma de BYTE: 200 + 100 = 44 (módulo 256)
w5 = b0 + b1        ' el destino es WORD: se calcula en 16 bits, 300
w6 = b0 * 3         ' 600
b3 = w6 / 4         ' calculado en 16 bits y truncado a BYTE: 150
b4 = 300            ' ERROR PYB0040: 300 no cabe en un BYTE
```

Una constante que no cabe en el destino es un error; una expresión más ancha que el destino se trunca con una conversión explícita en el IR (visible en el listado).

## Los pines: por su nombre, según el dispositivo

Un pin se escribe con el nombre del dispositivo: **`RA0`…`RA7`, `RB0`…`RB7`, `RC0`…`RC7`, `RD0`…`RD7`, `RE0`…`RE3`** (en mayúsculas o minúsculas: `RB0`, `rb0`, `Rb0`). La forma de PICAXE (`B.0`, `pinB.0`) y los nombres de `SYMBOL led = RB0` siguen valiendo. **Qué pines existen no es una lista fija: sale del Device Pack del PIC** (`pins.json`), así que el puerto E del PIC16F887 llega a `RE3` y `RE5` es un error que lo dice (`PYB0049`: «el puerto E tiene RE0, RE1, RE2, RE3»).

```basic
RB0 = 1               ' alto (como HIGH RB0); RB0 = 0 es LOW; con una variable, distinto de cero es alto
RD7 = x
IF RC0 = 1 THEN       ' leer un pin por su nombre
  TOGGLE RB1
ENDIF
READADC RA0, v        ' el canal ANn sale del pin: RA0 es AN0, RB0 es AN12...
```

Cada pin tiene un **tipo** según el dispositivo y según cómo el programa lo configura, y el compilador lo comprueba:

| Tipo | Cómo se usa | Error si no corresponde |
|---|---|---|
| **Digital, salida (OUT)** | `HIGH`, `LOW`, `TOGGLE`, `OUTPUT`, `RB0 = 1`: el pin pasa a salida y, si era analógico, a digital | — |
| **Digital, entrada (IN)** | `INPUT RB0`, `x = RB0`, `IF RB0 = 1`: se lee | — |
| **Solo entrada** (`RE3`/MCLR en el PIC16F887) | solo se lee | `PYB0053`: «RE3 es solo entrada» al escribirlo |
| **Analógico (ANALOG)** | `READADC RA0, v`: solo los pines con canal ANn; `READADC` deja el pin analógico | `PYB0066` si el pin no es analógico (la lista de los que sí lo son sale del dispositivo) |
| **Función de un periférico** (TX/RX de la UART: `SERTXD`, `SERRXD`, `BAUD`) | el periférico toma el pin | `PYB0054`: «RC6 es TX o CK de EUSART»: no es a la vez E/S digital |
| **Analógico y digital a la vez** | — | `PYB0054`: «RA0 es analógico (AN0) por READADC y también se usa como E/S digital» |

Los errores señalan la línea del primer uso del pin. Los pines que cada periférico toma salen de la tabla del dispositivo (`peripherals.json`), no de una lista del compilador. Los demás roles del dispositivo (comparadores, referencia, reloj, SDA/SCL del MSSP, PWM) los comprueba el mismo verificador que en Python (`PYP2150`, `PYP3010`).

## Sentencias

```basic
IF b0 > 5 AND b1 = 2 THEN      ' AND, OR, NOT sobre comparaciones; un valor suelto es "distinto de cero"
  HIGH led
ELSEIF b0 = 0 THEN
  LOW led
ELSE
  TOGGLE led
ENDIF                          ' o END IF
IF b0 = 1 THEN HIGH led ELSE LOW led      ' una línea
IF b0 = 1 THEN fin                        ' IF c THEN etiqueta  = salta a la etiqueta

FOR b0 = 1 TO 10 STEP 2 ... NEXT b0       ' constantes: cuenta hacia arriba; si el inicio es mayor, hacia abajo
DO WHILE b0 < 5 ... LOOP                  ' DO ... LOOP UNTIL c, DO ... LOOP (infinito), EXIT sale
WHILE b0 > 0 ... WEND

SELECT CASE b0
  CASE 1, 2
    ...
  CASE 5 TO 9
    ...
  CASE ELSE
    ...
END SELECT                                ' el sujeto es una variable

GOTO etiqueta        ' etiqueta:  (dentro del programa principal o de una subrutina)
GOSUB subrutina      ' subrutina:  ...  RETURN
END                  ' el programa se queda parado
```

El `FOR` suma o resta una constante y **no puede desbordar**: `FOR b0 = 0 TO 255` es un error (`PYB0054`) porque `255 + 1` no cabe en un BYTE; usa una variable WORD. Si el final no es constante, el compilador no puede comprobarlo.

**`GOSUB` son funciones.** Cada etiqueta a la que se hace `GOSUB` y que termina en `RETURN` se compila como una función (se llama con `CALL`, no con un salto), así que cuenta en la pila de hardware (8 niveles). Las subrutinas van después del programa principal (que termina en `END` o `GOTO`). Una subrutina no puede hacer `GOTO` a una etiqueta del programa principal ni al revés: un `GOTO` salta dentro de la misma función (`PYP2230`).

## Comandos al estilo PICAXE

| Comando | Qué hace |
|---|---|
| `HIGH p` / `LOW p` / `TOGGLE p` | pone el pin en alto, en bajo o lo invierte; el pin pasa a ser salida **al empezar el programa** (no en cada llamada) |
| `OUTPUT p` / `INPUT p` | `INPUT` lo deja como entrada digital |
| `PAUSE n` | espera `n` milisegundos exactos al ciclo (como `pause(n)` de Python); `n` puede ser una variable WORD o BYTE |
| `PAUSEUS n` | espera `n` microsegundos (`n` constante, como `delay_us`) |
| `READADC canal, b0` | lee el canal `ANn` y guarda los **8 bits de arriba** (de 10) en un BYTE |
| `READADC10 canal, w3` | lee los 10 bits en una variable WORD; el ADC se enciende solo al empezar |
| `READ dirección, b0` / `WRITE dirección, valor` | lee y escribe un byte de la **EEPROM de datos** (256 bytes); `WRITE` espera hasta 6 ms a que el chip termine |
| `PWMOUT C.2, periodo, ciclo` | PWM por el pin del módulo CCP (`C.2` es PWM1 y `C.1` es PWM2); `periodo` es PR2 (constante de 0 a 255, prescaler 1: periodo de PR2 + 1 ciclos de instrucción) y `ciclo` el ciclo de trabajo de 10 bits (0 a 1023). Los dos módulos comparten Timer2: un solo periodo por programa |
| `SERTXD("x = ", #b0, CR, LF, b1)` | envía por la UART (`BAUD`, 9600 por defecto, 8N1, RC6/TX; se inicia sola): textos, `#valor` en decimal, `CR`, `LF`, `TAB`, un byte constante y un valor de 8 bits suelto (se envía tal cual) |
| `SERRXD(b0)` | espera un byte por la UART (RC7/RX) y lo guarda en una variable BYTE |
| `pinB.0`, `B.0` | en una condición o expresión lee el pin (0 o 1): `IF pinB.0 = 1 THEN ...` |

`p` es un pin (`B.0`, `D.5`) o un nombre definido con `SYMBOL`. **No hay números de pin al estilo PICAXE** (el número cambia con el chip): el pin se nombra por su puerto y bit.

## Interrupciones

```basic
ON INTERRUPT aviso        ' la etiqueta aviso es la rutina de interrupcion
TMR2.every_ms 20
TMR2.interrupt_on         ' habilita la fuente (la interrupcion de Timer2)
ENABLE INTERRUPTS         ' GIE: DISABLE INTERRUPTS lo baja
main:
  GOTO main

aviso:
  TMR2.clear
  b0 = b0 + 1
  RETURN
```

`ON INTERRUPT etiqueta` convierte la etiqueta (que termina en `RETURN`) en la función `@interrupt` del PIC (guardado de contexto incluido). El PIC16 tiene **una sola** rutina de interrupción, y no se llama con `GOSUB`. Comparte las variables con el programa principal. Las demás reglas son las de Python: no se espera ni se llama a lo que también usa `main` (`PYP2060`). `ENABLE INTERRUPTS` sube `GIE`; la fuente se habilita aparte, con `TMRx.interrupt_on` o con un bit de registro: `INTCON.T0IE.set`, `PIE1.RCIE.set` (la forma `REGISTRO.BIT.set` o `.clear`).

## Ensamblador en línea

```basic
b0 = $A5
GOSUB intercambiar
END

ASM intercambiar
    SWAPF b0,F            ; ensamblador de pic-as: los comentarios van con punto y coma
    RETURN
END ASM
```

Un bloque `ASM nombre ... END ASM` es una rutina escrita en **ensamblador PIC-AS** (la misma sintaxis y el mismo mini-enlazador que `@asm_function` en Python, ver `docs/asm-inline.md`) que se llama con `GOSUB nombre`. Las variables se usan por su nombre (`b0`, `w1`, las de `DIM`; `b0` se escribe en minúsculas aunque el bloque diga `B0`, y una variable que solo nombra el bloque se declara) y el bloque debe terminar en `RETURN`. Va en el nivel principal del programa, no dentro de un `IF` o un bucle. Un error del ensamblador sale con la línea del `.bas`: `linea 7:5: error[ASM002]: instruccion o directiva desconocida 'FOOBAR'`. Valen las mismas reglas que en Python: no se llama desde la rutina de interrupción (`PYP2111`) y vuelve al banco 0 (`PYP2112`).

## El resto del SDK

Lo que no tiene un comando PICAXE (comparadores, CVREF, temporizadores, el resto de la UART y del PWM) se maneja con `OBJETO.método`, con o sin paréntesis, con los mismos nombres que en Python. Sin paréntesis es un comando: los valores van separados por comas y `nombre = valor` es un argumento con nombre.

```basic
CVREF.start step = 8                                  ' equivale a CVREF.start(step = 8)
CMP1.start negative = C12IN0, positive = REF_CVREF
TMR2.every_ms 20
UART.start baud = 4800                                ' en vez de BAUD, con UART.start la velocidad va aquí (no se combinan: PYB0109)
IF CMP1.out THEN HIGH led                             ' sin paréntesis también en una condición
IF TMR2.expired THEN
  TMR2.clear
ENDIF
w1 = ADC.read(AN0)                                    ' un método que devuelve un valor, en una expresión
```

Los nombres de los métodos y de los objetos no distinguen mayúsculas. Un método del SDK que devuelve un booleano (`out`, `expired`, `rx_ready`, `overrun`...) se usa directamente como condición. Los tipos de los argumentos los comprueba el verificador de siempre.

## Errores

Los errores propios de BASIC son `PYB0001`–`PYB0106` y salen con `archivo.bas:línea:columna`. Los demás (tipos, periféricos, pines, pila, interrupciones) son los `PYP...` de Python, con la posición en el `.bas`. Los principales:

| Código | Qué dice |
|---|---|
| `PYB0010`–`PYB0034` | sintaxis: algo sobra, falta `THEN`, `ENDIF`, `NEXT`, `LOOP`, `WEND`..., sentencia desconocida |
| `PYB0040` | una constante no cabe en el destino |
| `PYB0042` | negación (`-x`): los valores no tienen signo |
| `PYB0047` | variable sin declarar (`DIM x AS BYTE`) |
| `PYB0050` / `PYB0072` / `PYB0073` | `RETURN` fuera de una subrutina; `GOSUB` a una etiqueta que no existe; subrutina sin `RETURN` |
| `PYB0054` | el `FOR` desborda su variable |
| `PYB0062` | `HIGH`, `LOW`... necesitan un pin (`B.0`, o un nombre de `SYMBOL`) |
| `PYB0107`–`PYB0109` | `BAUD` repetido, no constante o combinado con `UART.start` |
| `PYB0074` | uso a la vez de `wN` y `b(2N)`/`b(2N+1)` (se solapan en PICAXE) |
| `PYP2230` / `PYP2231` | `GOTO` a una etiqueta que no está en la función / etiqueta repetida |

## Lo que todavía no está

La primera entrega es el núcleo y el SDK actual. Quedan para las siguientes (issue de seguimiento en GitHub):

- **Tablas en flash** (`DATA`/`LOOKUP`) y `SEROUT` con pin y velocidad propios.
- Bloques `ASM` con parámetros y valor de retorno (hoy son rutinas sin parámetros: se pasan los valores por variables).
- Los comandos PICAXE que todavía no existen: `SERIN` y `SEROUT` (con pin y velocidad propios), `PEEK`/`POKE`, `COUNT`, `PULSIN`, `SERVO`, `SOUND`, `DATA`/`LOOKUP`... Se escriben con los métodos del SDK (`UART.write`, `TMR1.value`...).
- `bit0`..`bit15`, `DIM x AS BYTE = 5` (valor inicial), arreglos de WORD, `SUB`/`FUNCTION` con parámetros, y que `w0` y `b0`/`b1` compartan memoria.
- La extensión de VS Code (resaltado, diagnósticos y botones para `.bas`).
