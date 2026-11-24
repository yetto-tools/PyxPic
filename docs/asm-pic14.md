# Ensamblador PIC14 (`pypic asm`): sintaxis de MPLAB XC8 PIC Assembler

## Desensamblado como fuente

`pypic disasm firmware.hex --pic-as -mcpu=16F887` escribe en la salida estándar instrucciones en sintaxis PIC-AS, con `PROCESSOR`, `PSECT` absoluto y `ORG` para conservar direcciones. El Device Pack permite identificar los vectores y emitir las palabras de configuración conocidas como `CONFIG CONFIGn = valor`. Las palabras no reconocidas o con codificaciones alternativas se conservan como `DW`; otras direcciones fuera de la memoria de programa se muestran como datos de zona especial. `--devices-dir` permite localizar Device Packs. Sin `-mcpu`, no se presupone un dispositivo y las zonas especiales se representan con `DW`. El formato sin `--pic-as` conserva el listado anterior.

El HEX no contiene nombres, tipos ni una separación fiable entre código y datos: la salida numérica no recupera el programa original. Se rotulan los vectores conocidos, pero los saltos y registros se mantienen numéricos porque la página y el banco dependen del estado en ejecución. `pypic asm` emite únicamente las palabras CONFIG mencionadas; compare las regiones relevantes al reensamblar con otros toolchains.

La prueba automatizada reensambla con `pypic asm` una imagen con direcciones dispersas y CONFIG. Además, se contrastó una imagen pequeña con vectores y ambas palabras CONFIG mediante PIC-AS 4.00 y el DFP oficial PIC16Fxxx 1.9.176: el HEX coincidió. Esta prueba no demuestra compatibilidad de todas las instrucciones, datos o zonas especiales. PIC-AS emitió un aviso sobre bits reservados de CONFIG2. Para ejecutar el ensamblador oficial se necesita el DFP correspondiente mediante `-mdfp`; no es una dependencia de PyPIC.

Para PIC16F887 se reconoce `CONFIG WRT = 256`, nombre del DFP que protege las primeras 256 palabras. No debe confundirse con el literal hexadecimal `0x100`: al no ser un nombre, se trata como número y se conservan solamente los bits del ancho del campo, igual que PIC-AS en las pruebas con WRT. Por ejemplo, `256` produce CONFIG2 `0x3DFF`, pero `0x100` produce `0x39FF`. Los números negativos se rechazan. Un número que exceda el campo se trunca y genera `warning[ASMW001]` con el valor aplicado.

**Estado: implementación parcial.** `pypic asm` ensambla un subconjunto de la sintaxis de `pic-as` para PIC16 clásico (MPLAB XC8 PIC Assembler, guía DS50002974F, cap. 4 y 5). No se garantiza compatibilidad completa: las directivas pendientes y las diferencias deben comprobarse con PIC-AS real. Es la base del ensamblador en línea ([asm-inline.md](asm-inline.md)).
Verificado: el blink escrito en PIC-AS (`examples/asm/blink_rd0.s`) produce **byte por byte** el mismo `.hex` que se probó en Proteus,
y las 16 008 instrucciones válidas sobreviven a desensamblar y volver a ensamblar.

### Comparación con XC8 PIC-AS

Se comparó `examples/asm/blink_rd0.s` con PIC-AS 4.00 y el DFP oficial PIC16Fxxx 1.9.176: ambos ensambladores produjeron la misma imagen, incluidas CONFIG1 y CONFIG2. Para repetir la prueba automatizada, indicar `PYPIC_TEST_PIC_AS` con la ruta de `pic-as.exe` y `PYPIC_TEST_XC8_DFP` con la carpeta `xc8` del DFP; ejecutar `cargo test -p pypic-cli --test cli pic_as_blink_matches_native_assembler_when_xc8_and_dfp_are_configured -- --nocapture`. Si faltan ambas variables la comparación se omite. Para un archivo `.s` con `#include`, el comando oficial necesita `-xassembler-with-cpp`; el DFP se indica con `-mdfp`. Esta prueba no certifica todas las directivas ni todos los valores de configuración.

```
pypic asm blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o blink_rd0.hex [--listing]
```

| Opción | Significado |
|---|---|
| `-mcpu=16F887` o `--device PIC16F887` | Dispositivo. Si falta se toma de la directiva `PROCESSOR`. Es necesario para `#include <xc.inc>` y `CONFIG` |
| `-Wl,-pNOMBRE=dir[,sig...]` | Opción del enlazador: coloca el psect `NOMBRE` en `dir` (los `sig` van a continuación). Se admiten varias, separadas por comas |
| `-Wl,--fixupoverflow=ignore` | Los operandos que no caben se truncan en vez de dar error (como en pic-as) |
| `-o`, `--listing`, `--devices-dir` | Salida, listado con direcciones y dónde buscar los Device Packs |

## Qué cubre

```asm
; comentario    // otro    /* otro */
PROCESSOR 16F887
#include <xc.inc>                       ; registros, campos y psects del dispositivo

CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, MCLRE = OFF

PSECT udata_bank0                       ; datos: la clase BANK0 reparte las direcciones
counter: DS 1

PSECT resetVec,class=CODE,delta=2       ; codigo: lo coloca -presetVec=0h
    goto main

PSECT code
main:                                   ; las etiquetas SIEMPRE llevan ':'
    BANKSEL TRISD                       ; 2 palabras: BCF/BSF STATUS,RP0 y RP1
    bcf BANKMASK(TRISD),TRISD_TRISD0_POSN
    bcf STATUS,STATUS_RP0_POSN
    movf counter,w                      ; destino w o f (tambien 0 o 1; sin destino es f)
    btfss RB5                           ; RB5 es una macro de xc.inc: PORTB,5
    goto $+2
    return
```

### Sentencias, números y expresiones

- Formato `etiqueta: mnemónico operandos ; comentario`; mnemónicos y directivas **sin distinguir mayúsculas**, símbolos **sí**. Sin restricciones de columna.
- Comentarios `;`, `//` y `/* */`.
- **Números**: decimal por defecto; `0x1F` o `1Fh` (hexadecimal, con dígito inicial: `0FFh`); `101B` (binario, la `B` en mayúscula); `17o`/`17q` (octal); `10d`; `'A'`.
  `0b101` no existe en PIC-AS y el error indica la forma correcta.
- **Expresiones**: se evalúan **de izquierda a derecha, sin precedencia** (`5 + 1 * 2` vale 12; los paréntesis cambian el orden). Operadores
  `+ - * / mod & | ^ << >> = <> < > <= >=` y en palabra (`and shl shr eq ne gt ge lt le not`), más `low high highword`. `$` es la dirección actual.
- **Destino** de las instrucciones de byte: `,w` o `,f`, o `,0` o `,1`; si se omite es `f`.
- **Operandos que no caben**: `f` son 7 bits y `CALL`/`GOTO` 11. Un registro de otro banco da `ASM004` salvo que se enmascare con `BANKMASK(símbolo)` tras
  `BANKSEL`; una etiqueta lejana, con `PAGEMASK(etiqueta)` tras `PAGESEL`, o con `fcall`/`ljmp`, que lo hacen todo. Con `--fixupoverflow=ignore` se trunca.

### Directivas

| Directiva | Estado |
|---|---|
| `PROCESSOR`, `PSECT`, `ORG`, `ALIGN`, `DB`, `DW`, `DS`, `EQU`, `SET`, `GLOBAL`/`PUBLIC`, `END` | ✅ |
| `CONFIG ajuste = valor[, ...]` y `CONFIG CONFIGn = valor` | ✅ (nombres del Device Pack, ver abajo) |
| `BANKSEL`, `BANKISEL`, `PAGESEL`, `fcall`, `ljmp` | ✅ (expanden a `BCF`/`BSF` de `STATUS` y `PCLATH`; `fcall` son 5 palabras y `ljmp` 3, como en la guía) |
| `IF`, `ELSIF`, `ELSE`, `ENDIF`; `#include <xc.inc>`, `#define NOMBRE valor`, `#undef`, `#ifdef`, `#ifndef`, `#if defined(X)`, `#else`, `#endif` | ✅ |
| `LIST`, `TITLE`, `NOLIST`, `FNCALL`, `ASMOPT`..., directivas de listado y de grafo de llamadas | ✅ se aceptan sin efecto |
| `MACRO`/`ENDM`, `LOCAL`, `REPT`, `IRP`, `IRPC`, `RADIX` | ✅ ver «Macros» |
| `DABS`, `DLABS`, `DDW`, `DQW`, `EXTRN`, `EXITM`, el flag `bit` de `PSECT` | ⏳ error `ASM008` explícito (#60) |

`DB`/`DW` en la memoria de programa escriben **una palabra por valor** (con `delta=2`, el byte alto en cero), como indica la guía.

### Macros

La expansión es **textual**, antes de ensamblar, y las líneas expandidas llevan el número de la línea de la invocación (un error dentro de una macro apunta a donde se la usa; el listado
muestra las palabras generadas bajo esa línea).

```asm
loadPort MACRO port,value        ; nombre MACRO parametros (sin ':')
    movlw value
    movwf PORT&port              ; '&' concatena: loadPort B,0x55 -> movwf PORTB (por eso el AND de bits se escribe 'and')
ENDM

down MACRO count
    LOCAL more                   ; un nombre unico (??0001, ??0002...) en cada expansion
more:
    decfsz count,f
    goto more
ENDM

    loadPort B,0x55
    down 0x20
    down 0x21                    ; sin etiquetas repetidas
    ld <0x20,w>                  ; <...> mete comas en un argumento: movf 0x20,w
REPT 3                           ; repite el bloque 3 veces (constante, hasta 4096)
    nop
ENDM
IRP number,1Ah,2Bh               ; una vez por valor
    DW number
ENDM
IRPC char,ABC                    ; una vez por caracter: DB 'A', 'B', 'C'
    DB 'char'
ENDM
RADIX hex                        ; dec, hex u oct: base de los numeros sin sufijo
```

- Una macro admite otras macros, `REPT`/`IRP`/`IRPC` anidados y `IF`/`ELSE`; una macro que se llama a sí misma sin fin se corta a los 32 niveles.
- Faltar argumentos los deja vacíos; pasar de más es un error. El nombre de una macro no puede ser un mnemónico ni una directiva.
- Un parámetro se sustituye también dentro de comillas (`'char'`), como en la guía.
- `EXITM` no se admite: la expansión no conoce las condiciones del ensamblado; usa `IF`/`ELSE` en la macro.
- `DDW`/`DQW`/`DABS` siguen sin implementar: la guía no fija cómo se reparten 32 o 64 bits en palabras de 14 bits ni el relleno del espacio absoluto, y no se inventan (#60).
- Dentro de una `@asm_function` no hay macros: el cuerpo es una sola función con las reglas de [asm-inline.md](asm-inline.md).

### `PSECT` y el mini-enlazador

pic-as no pone el código donde se escribe: cada `PSECT` lo coloca el enlazador. PyPIC lleva un enlazador mínimo con las mismas reglas:

1. **`abs`**: el psect empieza en la dirección 0 y `ORG` es una dirección absoluta (el uso clásico para vectores).
2. **`-pNOMBRE=dir[,sig...]`** lo fija en `dir`. Sin esa opción el psect `reset_vec` va en 0, seguido de `intentry`, `init` y `end_init`
   (el valor por defecto de pic-as: `-preset_vec=00h,intentry,init,end_init`).
3. **El resto** va al primer hueco libre de su **clase**: `CODE` (páginas de 0x800 palabras, un psect no las cruza), `BANK0`…`BANK3`, `COMMON`, `RAM`.
   Un psect sin clase y sin `-p` es un error, como en el enlazador.
4. `ORG` dentro de un psect relocalizable es un **desplazamiento** desde su comienzo y no puede retroceder.
5. Flags: `abs`, `ovrld`, `class=`, `delta=`, `space=`, `reloc=`, y se aceptan `size`, `limit`, `with`, `optim`, `merge`, `split`, `global`, `local`, `pure`, `noexec`, `note`. `bit` aún no.

`#include <xc.inc>` define los psects `code` (`CODE`), `data` (`STRCODE`), `udata` (`RAM`), `udata_bank0`…`udata_bank3` (`BANKn`) y `udata_shr` (`COMMON`);
sin el `#include`, `PSECT code` no tiene clase.

### `xc.inc` y los registros

Con `#include <xc.inc>` (o `INCLUDE <xc.inc>`) el Device Pack da: cada **registro** con su **dirección completa** (`TRISB` = `0x86`),
cada campo con `REGISTRO_CAMPO_POSN`/`_POSITION` (número de bit), `_SIZE`/`_LENGTH` y `_MASK`, y los **campos como macros** que valen `REGISTRO,bit`
(`RB5` → `PORTB,5`, así `btfss RB5`). Igual que en pic-as, un campo de una sola letra (`C`, `Z`) o repetido en varios registros no tiene macro:
se usa `STATUS_Z_POSN`.

### `CONFIG`

`CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF` o entre comillas `CONFIG "WDTE = OFF"`. El valor es **primero un nombre** del dispositivo (`ON`, `OFF`,
`XT`, `INTRC_NOCLKOUT`…) y solo si no lo es, una constante. Las palabras no mencionadas quedan en su valor por defecto (`0x3FFF`); `CONFIG CONFIG1 = 0x20D4`
fija la palabra entera. Se repite un ajuste: error. Los nombres están en `devices/<pic>/config.json` (`names`) y se validan contra los valores del datasheet.

> **A comprobar:** los nombres de valor del PIC16F887 (`INTRC_NOCLKOUT`, `BOR21V`, `1FOURTH`…) están tomados de las convenciones del DFP de Microchip; conviene
> contrastarlos con `pic_chipinfo.html` del DFP. Los de un solo bit `ON`/`OFF` siguen el significado ("`CP = ON` = protección de código activa").

## Las 35 instrucciones

| Forma | Instrucciones |
|---|---|
| `OP f[,w\|f]` | `ADDWF` `ANDWF` `COMF` `DECF` `DECFSZ` `INCF` `INCFSZ` `IORWF` `MOVF` `RLF` `RRF` `SUBWF` `SWAPF` `XORWF` |
| `OP f` | `CLRF` `MOVWF` |
| `OP f,bit` | `BCF` `BSF` `BTFSC` `BTFSS` |
| `OP k` (0..=255) | `ADDLW` `ANDLW` `IORLW` `MOVLW` `RETLW` `SUBLW` `XORLW` |
| `OP dirección` | `CALL` `GOTO` |
| sin operandos | `CLRW` `CLRWDT` `NOP` `RETFIE` `RETURN` `SLEEP` |

`MOVFW`, `SETC`, `BZ`… (pseudoinstrucciones de MPASM que pic-as no implementa) y `MOVLB`, `BRA`… (de otra familia, PIC16F1xxx) dan un error que explica qué escribir.

## Errores

Formato `archivo:línea:col: error[ASMnnn]: mensaje`; se informan **todos**, en orden. Los de MPASM llevan la forma de PIC-AS equivalente
(`__CONFIG` → `CONFIG`, `CBLOCK` → `PSECT` + `DS`, `loop` sin `:` → `loop:`, `0b101` → `101B`).

| Código | Regla |
|---|---|
| `ASM001` | Carácter o número inválido |
| `ASM002` | Instrucción o directiva desconocida, de otra familia, de MPASM o etiqueta sin `:` |
| `ASM003` | Cantidad o forma de operandos incorrecta |
| `ASM004` | Valor fuera de rango o desbordamiento del operando (falta `BANKMASK`/`PAGEMASK`) |
| `ASM005` | Símbolo no definido, repetido o reservado |
| `ASM006` | Dirección ocupada o fuera de rango, `ORG` hacia atrás, psects que se solapan o no caben |
| `ASM007` | Regla de una función `@asm_function` (ver [asm-inline.md](asm-inline.md)) |
| `ASM008` | Directiva o forma de pic-as todavía no soportada |
| `ASM009` | Código o datos fuera de un `PSECT` |
| `ASM010` | Flag, clase o delta de `PSECT` inválidos, o instrucción en un psect de datos |
| `ASM011` | `PROCESSOR` no coincide con el dispositivo |
| `ASM012` | `ERROR` del usuario |
| `ASM013` | `CONFIG`: ajuste o valor inexistente, repetido o sin dispositivo |

Dos sentencias en la misma dirección son siempre un error, aunque escriban el mismo valor.

## Diferencias deliberadas con pic-as

- Un solo archivo y un solo módulo: `EXTRN` y varios `.s` no están soportados (enlazar módulos sería otro proyecto).
- Sin el optimizador del ensamblador (pic-as tampoco optimiza: `ASMOPT` se ignora).
- El orden de los dos `BCF`/`BSF` de `BANKSEL` es RP0 y después RP1; la guía no fija el orden y no se ha comprobado byte a byte con un `pic-as` real
  (el `fcall` sí reproduce el ejemplo de la guía).
