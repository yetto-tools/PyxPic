# Ensamblador PIC14 (`pypic asm`)

**Estado: implementado.** Convierte texto ensamblador del PIC16 clásico en palabras de 14 bits y en Intel HEX, en dos pasadas. Es la base del ensamblador en línea (#42)
y la herramienta para escribir a mano lo que el lenguaje todavía no cubre. Verificado: el blink escrito en ensamblador (`examples/asm/blink_rd0.asm`) produce **byte por byte** el mismo
`.hex` que se probó en Proteus, y las 16 008 instrucciones válidas sobreviven a desensamblar y volver a ensamblar.

```
pypic asm examples/asm/blink_rd0.asm -o blink.hex [--listing]
```

## Sintaxis (una sola forma por operación)

```asm
; comentario
STATUS  EQU 0x03          ; constantes: solo con EQU, y solo con simbolos ya definidos
RP0     EQU 5
        ORG 0x0005        ; direccion de palabra
main:   BSF  STATUS,RP0   ; etiqueta y instruccion en la misma linea, o en lineas aparte
        ADDWF 0x20,W      ; el destino es SIEMPRE W o F (no 0 ni 1)
        GOTO $            ; $ = direccion actual; tambien $+1, etiqueta+2...
        ORG 0x2007
        DW 0x20D4, 0x3FFF ; palabras de 14 bits (aqui, las de configuracion)
```

- Mnemónicos y directivas **sin** distinguir mayúsculas; símbolos y etiquetas **sí**.
- Números: decimal, `0x1F`, `0b101`. Expresiones: términos con `+` y `-`.
- Directivas: `ORG`, `DW`, `EQU`. No hay `END`, macros ni `BANKSEL`: el banco se cambia con `BSF`/`BCF` sobre `RP0`/`RP1`.
- `f` son 7 bits (0..=0x7F): para un registro de otro banco se selecciona el banco y se usa el desplazamiento (ANSEL `0x188` → `0x08` en el banco 3).
- `CALL`/`GOTO` solo alcanzan la página 0 (`0..=0x7FF`); el ensamblador no carga `PCLATH` (#31).

## Las 35 instrucciones

| Forma | Instrucciones |
|---|---|
| `OP f,W|F` | `ADDWF` `ANDWF` `COMF` `DECF` `DECFSZ` `INCF` `INCFSZ` `IORWF` `MOVF` `RLF` `RRF` `SUBWF` `SWAPF` `XORWF` |
| `OP f` | `CLRF` `MOVWF` |
| `OP f,bit` | `BCF` `BSF` `BTFSC` `BTFSS` |
| `OP k` (0..=255) | `ADDLW` `ANDLW` `IORLW` `MOVLW` `RETLW` `SUBLW` `XORLW` |
| `OP direccion` | `CALL` `GOTO` |
| sin operandos | `CLRW` `CLRWDT` `NOP` `RETFIE` `RETURN` `SLEEP` |

## Errores

Formato `archivo:linea:col: error[ASMnnn]: mensaje`; se informan **todos**, en orden. Las instrucciones que no existen en este dispositivo explican por qué
(`MOVLB`, `BRA`... son de otra familia, PIC16F1xxx; `SETC`, `MOVFW`... son pseudoinstrucciones de MPASM).

| Código | Regla |
|---|---|
| `ASM001` | Carácter o número inválido |
| `ASM002` | Instrucción o directiva desconocida, de otra familia o pseudoinstrucción |
| `ASM003` | Cantidad o forma de operandos incorrecta (incluye destino `0`/`1` o ausente) |
| `ASM004` | Valor fuera de rango (registro, bit, literal, palabra) |
| `ASM005` | Símbolo no definido, repetido o reservado |
| `ASM006` | Dirección fuera de rango o ya ocupada, o salto fuera de la página 0 |

Dos sentencias en la misma dirección son siempre un error, aunque escriban el mismo valor.
