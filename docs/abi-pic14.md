# ABI PIC14 (PIC16 clásica) — versión 0.1

**Estado: especificación aprobada, backend NO implementado.** Solo las reglas marcadas ✅ ya se comprueban en el frontend.
Fuente de hardware: **DS40001291H** (PIC16F882/883/884/886/887). Las páginas citadas son de ese documento.
Todo es código nativo generado en el host; no hay intérprete ni VM de Python en el MCU.

## 1. Hechos del hardware que condicionan la ABI

| Hecho | Página |
|---|---|
| Un solo acumulador `W`; las instrucciones operan sobre un registro `f` y eligen destino con el bit `d` (W o f). 35 instrucciones, todas de 1 ciclo salvo saltos (2) | 1, 227 |
| `CALL`/`GOTO` llevan solo 11 bits de dirección; los bits 12:11 del PC salen de **PCLATH<4:3>** (páginas de 2 K palabras; la F887 tiene 4) | 38 |
| Pila de retorno: **8 niveles × 13 bits**, circular, **sin indicación de desbordamiento ni subdesbordamiento**; PCLATH no cambia con CALL/RETURN | 38 |
| Datos en 4 bancos seleccionados con **RP1:RP0** de STATUS (acceso directo) e **IRP** (indirecto) | 23, 30 |
| `70h–7Fh` es RAM común, visible en todos los bancos (espejos en F0h, 170h, 1F0h) | 23, 26 |
| Al atender una interrupción solo se guarda el PC; **W, STATUS y PCLATH los guarda el software**; se recomienda guardarlos en la RAM común y usar `SWAPF` porque no altera STATUS | 219 |
| GIE se desactiva al entrar a la interrupción (no hay anidamiento); `RETFIE` la reactiva. Latencia 3–4 ciclos | 216, 218 |
| Vector de reset `0000h`, vector de interrupción `0004h` | 22 |
| STATUS: solo `BCF`, `BSF`, `SWAPF` y `MOVWF` lo modifican sin efectos colaterales | 30 |

## 2. Tipos y representación

`uint8 int8 bool` = 1 byte; `uint16 int16` = 2 bytes; `uint32 int32` = 4 bytes. **Little-endian** (byte bajo en la dirección menor); enteros con
signo en complemento a dos; `bool` es 0 o 1. La aritmética es modular en el ancho del tipo (ver `docs/frontend.md`).
Los tipos más anchos que 8 bits se compilan con secuencias nativas (cadenas de acarreo) o rutinas de runtime (§7).

## 3. Memoria estática

- No hay pila de datos: globales, parámetros, locales y temporales son **posiciones fijas** que decide el backend.
- **Solapamiento (overlay)** de locales entre funciones que nunca están activas a la vez, según el grafo de llamadas. Es seguro porque no hay
  recursión ✅ (`PYP2013`) y porque ninguna función se comparte entre `main` y la interrupción ✅ (`PYP2060`): el contexto principal y el de
  la ISR usan **áreas de memoria disjuntas**.
- Los globales se inicializan con constantes (`MOVLW k` / `MOVWF f`) en el arranque; no hay copia desde flash.
- Reparto de la RAM común `70h–7Fh` (16 bytes): `70h W_TEMP`, `71h STATUS_TEMP`, `72h PCLATH_TEMP`, `73h FSR_TEMP` (la interrupcion lo usa para guardar `FSR` solo si ella o algo que llama usa arreglos);
  `74h–7Fh` libres para temporales calientes del compilador.

## 4. Convención de llamada

- **Argumentos:** el llamador escribe cada argumento en la posición estática del parámetro del llamado, antes del `CALL`.
- **Retorno:** `uint8`/`int8`/`bool` en **W**. Valores de 16/32 bits: byte bajo en W y el resto en `RETV1..RETV3` (posiciones estáticas del **contexto** del llamado).
- **Banco (decisión):** **invariante de banco 0** — toda función entra y sale con RP1:RP0 = 00 y IRP = 0. Quien use otro banco lo restaura antes de `RETURN`.
- **PCLATH:** como `CALL`/`GOTO` siempre toman PCLATH<4:3>, antes de cada llamada se carga PCLATH con la página destino y al volver se restablece la actual.
  Invariante en los límites de función: PCLATH<4:3> = página del código en ejecución. Un programa que cabe en **una página (2048 palabras)** no hace nada de esto.
  Uno mayor se genera en el **modo de varias páginas** (ver más abajo).

### Programas de más de una página (#31)

`CALL` y `GOTO` llevan 11 bits; los bits 12:11 del destino salen de PCLATH<4:3> (DS40001291H p.38). Cuando el programa no cabe en 2048 palabras el compilador lo vuelve a generar así:

- **Cada función entra entera en una página**: se coloca en la siguiente si cruzaría el límite de 0x800, y una función de más de 2048 palabras es un error (`FunctionTooLarge`). Por eso los saltos
  dentro de una función no necesitan PCLATH. Lo mismo vale para cada rutina de runtime (`rt.*`).
- **Cada llamada entre funciones** (también a rutinas de runtime) es `BCF/BSF PCLATH,4` · `BCF/BSF PCLATH,3` · `CALL` · `BCF/BSF PCLATH,4` · `BCF/BSF PCLATH,3`: **5 palabras y +4 ciclos**, sin tocar W (que lleva el valor
  devuelto), con las páginas resueltas al enlazar. Se paga también cuando origen y destino están en la misma página: el tamaño de cada sitio es fijo, así no hace falta iterar el enlace.
- **Interrupción**: el `GOTO` del vector `0004h` usaría PCLATH de quien fue interrumpido, así que el **prólogo va en el propio vector** (guarda W, STATUS y PCLATH, y limpia PCLATH) y después carga la página de
  la ISR y salta a ella. El arranque pasa a ir a continuación del prólogo. La ISR puede estar en cualquier página.
- **Tablas `Rom`**: la consulta ya cargaba PCLATH completo (incluidas las páginas); al volver restablece solo los bits de página (**18 ciclos** en lugar de 17).
- Más allá del tamaño de la flash del dispositivo es `TooLarge`. Para un programa chico no cambia ni una palabra del código generado.

## 5. Interrupciones

Entrada en `0004h`, siguiendo el ejemplo 14-1 (p.219), sin cambiar STATUS antes de guardarlo:

```
MOVWF  W_TEMP          ; W a la RAM común (funciona en cualquier banco)
SWAPF  STATUS,W        ; SWAPF no altera STATUS
MOVWF  STATUS_TEMP
MOVF   PCLATH,W        ; PCLATH existe en todos los bancos
MOVWF  PCLATH_TEMP
CLRF   PCLATH          ; el cuerpo de la ISR se enlaza en la página 0
BCF    STATUS,RP0      ; banco 0 (y RP1, IRP) para el cuerpo
...                    ; cuerpo de la funcion @interrupt
MOVF   PCLATH_TEMP,W
MOVWF  PCLATH
SWAPF  STATUS_TEMP,W   ; restaura STATUS y los bits de banco
MOVWF  STATUS
SWAPF  W_TEMP,F
SWAPF  W_TEMP,W        ; restaura W sin tocar STATUS
RETFIE
```

El cuerpo de la ISR debe borrar sus propios flags (responsabilidad del SDK). `FSR` se guarda/restaura solo si la ISR usa direccionamiento indirecto.

## 6. Arranque y pila

- `0000h: GOTO startup` (tras el reset PCLATH = 0). `0001h–0003h` libres.
- `startup`: pone banco 0, inicializa globales, `CALL` a la función `@init_device` (si existe), `CALL main`, y si `main` vuelve, `SLEEP` + salto a sí mismo. **Cada `CALL` desde `startup` consume 1 nivel de pila** (son secuenciales: cuenta el mayor de los dos).
- ✅ **Profundidad de pila (`PYP2061`, decisión: error):** peor caso = `1 + profundidad(main)` + `1 + profundidad(ISR)` ≤ niveles del pack (8 en la F887).
  La entrada a la interrupción empuja el PC (1 nivel). El frontend cuenta también las rutinas de runtime y las consultas de tablas `Rom` (un nivel cada una; la división con signo, dos).

## 7. Rutinas de runtime

Multiplicación, división, módulo y desplazamientos por variable son rutinas nativas estáticas, nunca un intérprete. Como usan memoria estática,
**(decisión) el compilador genera una copia por contexto** (principal / ISR) si ambos la usan: solo cuesta flash y 9 bytes de RAM por contexto en ese caso. Implementado: ver `docs/sdk-runtime.md`.

## 8. Qué está implementado

| Regla | Estado |
|---|---|
| Sin recursión (`PYP2013`) | ✅ frontend |
| Ninguna función compartida entre `main` e ISR (`PYP2060`) | ✅ frontend |
| Profundidad de pila de retorno (`PYP2061`) | ✅ frontend (solo llamadas del usuario) |
| Codificación/decodificación de las 35 instrucciones (`crates/pyxpic-pic14`) | ✅ verificada contra la tabla 15-2 (p.227), exhaustiva sobre las 16 384 palabras |
| Escritor/lector de Intel HEX (palabra → 2 bytes little-endian, dirección de bytes = 2 × dirección de palabra; config en `400Eh`/`4010h`) | ✅ vectores calculados a mano + ida y vuelta (`crates/pyxpic-pic14`); **pendiente de verificar en Proteus** |
| Convención de llamada, invariante de banco 0 (todo el dato en banco 0), temporales por contexto, prólogo/epílogo de ISR, arranque | ✅ `codegen` (ver `docs/backend-pic14.md`); verificado con pruebas diferenciales IR ↔ simulador PIC14 e interrupciones inyectadas |
| Rutinas de runtime (`*`, `//`, `%`, desplazamientos variables), una copia por contexto | ✅ `docs/sdk-runtime.md` |
| Programas de más de 2048 palabras (PCLATH, una función por página, prólogo en el vector) | ✅ `crates/pyxpic-pic14/tests/pages.rs` |
| Overlay de locales y otros bancos de datos | ⏳ backend (#26) |
| Re-comprobación de pila con rutinas de runtime | ⏳ backend |
