# Generador de código PIC14 (`pypic-pic14::codegen`)

**Estado: primer corte implementado.** IR tipada → instrucciones PIC14 nativas → Intel HEX. Todo corre en el host; el MCU solo
ejecuta las instrucciones generadas (sin intérprete ni VM). Reglas de la ABI: `docs/abi-pic14.md`.

```
pypic build programa.py -o salida.hex [--listing] [--device devices/pic16f887]
```

## Qué soporta

- Tipos `u8`, `i8`, `bool`, `u16`, `i16`. Little-endian, aritmética modular.
- `+ - & | ^ ~`, negación, `not`, comparaciones (con y sin signo, 8 y 16 bits), desplazamientos por **constante**, conversiones.
- `if/elif/else`, `while`, `for range`, `break/continue`, `and/or/not` con cortocircuito.
- Funciones con argumentos y retorno (8/16 bits), globales inicializados en el arranque, `@interrupt` con guardado de contexto.
- Pines: `RD0.output()/input()/on()/off()/toggle()/value()` (limpia `ANSEL`/`ANSELH`, fija `TRISx`, vuelve al banco 0).
- Programas con `main()`; solo se compilan las funciones alcanzables desde `main` o la ISR.

## Cómo está generado

| Aspecto | Decisión |
|---|---|
| Memoria | Todo en el banco 0 (`20h–6Fh`, 80 bytes): globales, parámetros, locales y temporales, sin solapamiento. Así se cumple la invariante de banco 0 |
| Zona común `70h–7Fh` | `70h W_TEMP`, `71h STATUS_TEMP`, `72h PCLATH_TEMP`; `74h–77h` temporales del contexto principal, `78h–7Bh` de la ISR; `7Ch`/`7Dh` byte alto del retorno de 16 bits (principal/ISR) |
| Llamada | Argumentos a las posiciones del llamado, `CALL`, retorno en W (byte bajo) y `RETV` (byte alto) |
| Comparaciones | `SUBWF`/`SUBLW` y bit C; con signo se invierte el bit 7 (`XORLW 0x80`); 16 bits: primero el byte alto, y el bajo solo si empatan |
| Arranque | `0000h: GOTO startup`; `startup` inicializa las variables globales (la RAM no está definida tras el reset; las constantes `Const` no ocupan RAM), pone a 0 los latches de los puertos usados, llama a la función `@init_device` (si existe), luego a `main`, y duerme |
| Parámetros del dispositivo | RAM útil del banco 0, tamaño de programa y direcciones de CONFIG vienen del Device Pack (`Options`); el backend comprueba además que el dispositivo declarado coincida |
| Interrupción | `0004h: GOTO isr` (o `RETFIE` si no hay ISR); prólogo/epílogo del ejemplo 14-1 del datasheet (p.219) |
| Configuración | `CONFIG1 = 20D4h`, `CONFIG2 = 3FFFh` (INTOSCIO, WDT apagado, MCLR interno). TODO: tomarlos del Device Pack |

## Límites actuales (el compilador los rechaza con un error claro)

- Solo genera código para el **PIC16F887**: el mapa de registros (PORTx, TRISx, ANSEL) está fijo en el backend (#22). Con otro dispositivo, o si el programa declara uno distinto del compilado, da un error en vez de código equivocado.
- `*`, `//`, `%` sobre más de 16 bits (`u32`/`i32`, #32).
- `u32` / `i32`.
- Más de 80 bytes de datos: no hay otros bancos ni solapamiento de locales (#26).
- Más de 2048 palabras: no se maneja PCLATH.
- Acceso a pines (`pypic.periph.gpio`, ver `docs/sdk-gpio.md`) solo en los puertos A–E; sin retardos (`sleep_ms`) todavía.

## Cómo se verifica

`crates/pypic-pic14/tests/codegen.rs`: pruebas **diferenciales**. El mismo programa se ejecuta en el intérprete de referencia de la IR
(el oráculo) y, ya compilado, en el simulador del núcleo PIC14; los globales finales deben coincidir.

- Más de 3 000 casos de `+ - & | ^` y más de 4 000 de comparaciones, en `u8/i8/u16/i16`, con operandos límite y aleatorios (semilla fija).
- Constantes en cualquier operando, desplazamientos, conversiones, bucles, llamadas anidadas, cortocircuito.
- **Una interrupción inyectada tras cada instrucción posible de `main`** no altera su resultado (verifica el guardado de W, STATUS y PCLATH).

El simulador es del host y no modela periféricos; **no sustituye a Proteus ni al hardware**.
