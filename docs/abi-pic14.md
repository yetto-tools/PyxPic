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

`u8 i8 bool` = 1 byte; `u16 i16` = 2 bytes; `u32 i32` = 4 bytes. **Little-endian** (byte bajo en la dirección menor); enteros con
signo en complemento a dos; `bool` es 0 o 1. La aritmética es modular en el ancho del tipo (ver `docs/frontend.md`).
Los tipos más anchos que 8 bits se compilan con secuencias nativas (cadenas de acarreo) o rutinas de runtime (§7).

## 3. Memoria estática

- No hay pila de datos: globales, parámetros, locales y temporales son **posiciones fijas** que decide el backend.
- **Solapamiento (overlay)** de locales entre funciones que nunca están activas a la vez, según el grafo de llamadas. Es seguro porque no hay
  recursión ✅ (`PYP2013`) y porque ninguna función se comparte entre `main` y la interrupción ✅ (`PYP2060`): el contexto principal y el de
  la ISR usan **áreas de memoria disjuntas**.
- Los globales se inicializan con constantes (`MOVLW k` / `MOVWF f`) en el arranque; no hay copia desde flash.
- Reparto de la RAM común `70h–7Fh` (16 bytes): `70h W_TEMP`, `71h STATUS_TEMP`, `72h PCLATH_TEMP`, `73h FSR_TEMP` (reservado para indirecto);
  `74h–7Fh` libres para temporales calientes del compilador.

## 4. Convención de llamada

- **Argumentos:** el llamador escribe cada argumento en la posición estática del parámetro del llamado, antes del `CALL`.
- **Retorno:** `u8`/`i8`/`bool` en **W**. Valores de 16/32 bits: byte bajo en W y el resto en `RETV1..RETV3` (posiciones estáticas del **contexto** del llamado).
- **Banco (decisión):** **invariante de banco 0** — toda función entra y sale con RP1:RP0 = 00 y IRP = 0. Quien use otro banco lo restaura antes de `RETURN`.
- **PCLATH:** como `CALL`/`GOTO` siempre toman PCLATH<4:3>, antes de cada salto/llamada se carga PCLATH con la página destino (omitible si es
  la misma que la del código en curso). Al volver, el código sigue en su página. Invariante en los límites de función: PCLATH<4:3> = página del código en ejecución.

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
- `startup`: pone banco 0, inicializa globales, `CALL main`, y si `main` vuelve, `SLEEP` + salto a sí mismo. **El `CALL main` consume 1 nivel de pila.**
- ✅ **Profundidad de pila (`PYP2061`, decisión: error):** peor caso = `1 + profundidad(main)` + `1 + profundidad(ISR)` ≤ niveles del pack (8 en la F887).
  La entrada a la interrupción empuja el PC (1 nivel). *Limitación:* el frontend solo cuenta llamadas del usuario; las llamadas a rutinas de runtime se
  re-comprobarán en el backend.

## 7. Rutinas de runtime

Multiplicación, división, módulo y desplazamientos por variable son rutinas nativas estáticas, nunca un intérprete. Como usan memoria estática,
**(decisión) el compilador genera una copia por contexto** (principal / ISR) si ambos la usan; el usuario no ve nada y solo cuesta flash en ese caso.

## 8. Qué está implementado

| Regla | Estado |
|---|---|
| Sin recursión (`PYP2013`) | ✅ frontend |
| Ninguna función compartida entre `main` e ISR (`PYP2060`) | ✅ frontend |
| Profundidad de pila de retorno (`PYP2061`) | ✅ frontend (solo llamadas del usuario) |
| Codificación/decodificación de las 35 instrucciones (`crates/pypic-pic14`) | ✅ verificada contra la tabla 15-2 (p.227), exhaustiva sobre las 16 384 palabras |
| Escritor/lector de Intel HEX (palabra → 2 bytes little-endian, dirección de bytes = 2 × dirección de palabra; config en `400Eh`/`4010h`) | ✅ vectores calculados a mano + ida y vuelta (`crates/pypic-pic14`); **pendiente de verificar en Proteus** |
| Asignación de memoria y overlay, convención de llamada, banco 0, PCLATH, prólogo/epílogo de ISR, arranque, rutinas de runtime | ⏳ backend (#14, #15, #16 y los issues de seguimiento) |
| Re-comprobación de pila con rutinas de runtime | ⏳ backend |
