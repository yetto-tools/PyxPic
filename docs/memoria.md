# `pypic memory`: cuánta memoria usa el programa

```
pypic memory programa.py                      # el informe en texto
pypic memory programa.bas --details           # y el detalle por función y por variable
pypic memory --format json                    # un solo documento JSON versionado
pypic memory --project pypic.toml --target x  # con un proyecto, igual que build
```

Compila el programa como `pypic build` (mismas opciones, el mismo perfil de proyecto, los mismos errores) pero **no escribe ningún `.hex`** y no necesita XC8 ni gpasm. Sirve en Python y en BASIC. Si el programa no compila, sale el error de siempre y ningún informe (código 1); sin dispositivo en el programa ni en el perfil no hay capacidades que informar.

```
Memoria de PIC16F887 (examples/python/gpio/blink.py)
  Programa (flash):  27 de 8192 palabras de 14 bits (0,3 %), 8165 libres; dirección más alta usada: 0x001D
                     arranque y vectores: 7 palabras; configuración: 2 palabras (no cuentan como programa)
  RAM de datos:      5 de 80 bytes (6,2 %) en el banco 0, el único que usa el compilador por ahora; el chip tiene 368 bytes y la zona común reserva 16
  Pila de hardware:  1 de 8 niveles en el peor caso (principal 1, interrupción 0)
  Periféricos:       ninguno
  Pines usados:      RD0
```

## Qué se cuenta y cómo

| Dato | De dónde sale |
|---|---|
| **Capacidad** de programa, bits por palabra y niveles de pila | `memory.json` del Device Pack (PIC16F887: 8192 palabras de 14 bits, 8 niveles) |
| **Palabras usadas** | las palabras **realmente presentes** en la imagen que el compilador genera y que están dentro de la flash: los huecos (la alineación de las tablas, el salto al vector) no cuentan como ocupados. Coincide con las palabras del `.hex` que escribe `pypic build` (una prueba lo comprueba) |
| **Configuración** | las palabras de configuración y de identificación (`0x2000` en adelante) se informan aparte y **no** cuentan como programa |
| **Dirección más alta usada** | la última palabra ocupada de la flash + 1: con huecos, es mayor que las palabras usadas |
| **Por función** (`--details`) | las palabras ocupadas desde el punto de entrada de cada función hasta el de la siguiente; lo anterior a la primera es el arranque y los vectores. Las tablas y rutinas de apoyo que el compilador coloca al final se suman a la última función |
| **RAM** | los bytes que el compilador reservó (`ram_bytes`): variables globales y arreglos más locales, temporales y espacio de trabajo (esperas, rutinas de `*` `//` `%`). El compilador **solo usa el banco 0** (80 bytes, de 20h a 6Fh): el resto del chip y la zona común (70h a 7Fh, que usa la ABI para guardar el contexto) se informan como reserva, no como usados (#26) |
| **Pila de hardware** | el peor caso del verificador: la llamada de arranque, la cadena de llamadas más profunda del programa principal y, aparte, la de la interrupción con su entrada. Es el mismo cálculo del error `PYP2061` |
| **Periféricos y pines** | los que el programa usa (ADC, UART, EEPROM, PWM, comparadores/CVREF, temporizadores; los pines importados o nombrados) |

## Resultados parciales

Si el programa tiene funciones en **ensamblador** (`@asm_function` o un bloque `ASM`), sus palabras de programa se cuentan (están en la imagen), pero su uso de la pila de hardware y de la RAM por nombre **no se analiza**: el informe lo dice (`RESULTADO PARCIAL` en el texto; `"complete": false` y `notes` en el JSON) en vez de presentarlo como comprobado.

## El JSON (versión 1)

```json
{
  "version": 1, "tool": "pypic memory", "file": "...", "device": "PIC16F887",
  "complete": true, "notes": [],
  "program": { "unit": "words", "word_bits": 14, "capacity": 8192, "used": 27, "free": 8165, "used_percent": 0.33,
               "highest_address_used": 30, "startup_and_vectors": 7, "config_words": 2 },
  "ram": { "unit": "bytes", "chip_total": 368, "compiler_usable": 80, "used": 5, "globals": 0, "locals_and_workspace": 5,
           "free": 75, "used_percent": 6.25, "reserved_common": 16,
           "banks": [ { "bank": 0, "gpr_bytes": 96, "compiler_capacity": 80, "used": 5 }, ... ] },
  "stack": { "levels": 8, "worst_case": 1, "main": 1, "interrupt": 0 },
  "peripherals": [], "pins": ["RD0"],
  "details": { "functions": [ { "name": "main", "address": 14, "words": 16 } ], "variables": [ ... ] }
}
```

- **Compatibilidad:** `version` sube solo si cambia el significado de un campo o se quita uno; agregar campos no la cambia, así que una herramienta debe ignorar los que no conoce. `details` solo está con `--details`. `stack.worst_case` es `null` si el destino no declara sus niveles de pila.
- Con `--format json` la salida estándar es **un solo documento**: los avisos del compilador salen por la salida de errores.
- Las unidades van en el propio documento (`unit`): palabras de `word_bits` bits para el programa y bytes para la RAM, para que otro dispositivo con otro ancho de palabra no se confunda.

## Lo que no hace

- No estima el tamaño de lo que no compiló ni predice el de otro dispositivo: cada informe es de **un** dispositivo y de lo que el compilador generó para él.
- No reparte la RAM por banco más allá del banco 0 mientras el compilador no use los otros (#26); no modela el *overlay* de variables (también #26).
- La memoria de programa que se reserva con `CONFIG`/`IDLOC` en un `.s` o un `ORG` lejano de un programa en ensamblador (`pypic asm`) no pasa por este comando: es para programas de Python y de BASIC.
