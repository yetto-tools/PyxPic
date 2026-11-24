# Texto a número: `parse_dec` y `parse_hex`

**Estado: implementado (cuarta parte de #63).** Convierten el texto de un arreglo `uint8[N]` (por ejemplo la línea que dejó `UART.read_line`) a un `uint16`. La otra dirección, número a texto en decimal o hexadecimal, son las f-strings (`docs/sdk-uart.md`: `{v}`, `{v:04X}`). Juntas dan el convertidor decimal ↔ hexadecimal por el puerto serie (`examples/uart/convertidor.py`).

```python
linea: uint8[12]

n: uint8 = UART.read_line(linea)
error: bool = False
valor: uint16 = parse_dec(linea, n, error)      # "31"   -> 31
valor = parse_hex(linea, n, error)              # "0x1F" -> 31, "ff" -> 255
if error:
    UART.write_text("ERR\r\n")
else:
    UART.write_text(f"{valor} = 0x{valor:04X}\r\n")
```

Están **predeclaradas** (sin imports). Una función tuya con el mismo nombre tiene prioridad.

## Contrato

`parse_dec(buf, n, error)` y `parse_hex(buf, n, error)` leen los **primeros `n` bytes** de `buf` (`n` se acota a la capacidad del arreglo: nunca se lee fuera) y devuelven un `uint16`.

| | `parse_dec` | `parse_hex` |
|---|---|---|
| Admite | Solo cifras `0`–`9`, al menos una | Cifras `0`–`9`, `A`–`F`, `a`–`f`, al menos una; **prefijo opcional** `0x` o `0X` |
| Ceros a la izquierda | Sí (`0042` es 42) | Sí (`00001F` es 31) |
| Rango | 0 a 65535 | 0 a `0xFFFF` |
| **Error** | Cualquier otro byte (espacios, signo `-`/`+`, punto, coma, `NUL`), texto vacío, o más de 65535 | Lo mismo, y `0x` solo (sin cifras) |

**El error:** el lenguaje no tiene tuplas, así que no se puede devolver el valor y el error juntos. El tercer argumento es el **nombre de una variable `bool`** (global o local) donde la función anota el resultado. Con error, devuelve **0** y la variable queda en **`True`**; sin error queda en **`False`**: **siempre se escribe**, así un `True` viejo no se queda. Si el tercer argumento no es una variable `bool` es el error `PYP2189`, y una global necesita `global` en la función, como cualquier asignación (`PYP2023`).

No hay números negativos ni espacios alrededor: `" 7"`, `"7 "` y `"-5"` son error (quita los espacios antes, o recorta `n`). `read_line` ya quita el `\r` y el `\n`.

## Cómo está hecho y cuánto cuesta

Se desarman en la IR en un bucle de aritmética de **16 bits** (emulada por software: `PYP3001`), sin rutina de runtime ni intérprete. El desbordamiento se comprueba **antes** de multiplicar (decimal: `v > 6553`, o `v == 6553` y la cifra `> 5`; hexadecimal: `v > 0FFFh`), así que nunca se calcula un valor que no cabe. El decimal usa `v*10 = (v<<3) + (v<<1)`, sin multiplicación. El tiempo depende del largo del texto, **medido en el simulador** (ciclos de instrucción; a 4 MHz cada uno es 1 µs): `parse_dec` tarda 150 ciclos con 1 cifra (`"7"`) y 564 con 5 (`"65535"`); `parse_hex`, 154 con 1 (`"F"`) y 450 con 4 (`"FFFF"`): unos **100 ciclos por cifra**.

**Memoria:** el código de una conversión ocupa **cientos de palabras** de flash y los temporales (índice, valor, cifra...) se **reutilizan en toda la función**. Un programa que lee una línea, convierte en las dos bases y responde con f-strings (`convertidor.py`) usa **1286 palabras y 53 de los 80 bytes** de RAM del banco 0. El arreglo de texto es RAM fija.

## Lo que no hay (todavía)

- **Con signo** (`int16`) y números con **punto decimal**.
- **Otras bases** (binario `0b`, octal) y **espacios** a los lados.
- **Un desplazamiento** en el arreglo (convertir desde el byte `k`): se convierte siempre desde el índice 0.
- **`uint8` directo:** el resultado es `uint16`; conviértelo con `uint8(...)` si cabe (el compilador no comprueba que quepa).
- `format_into` (construir texto en un arreglo) y el tipo `char`: siguen pendientes en #63.

## Verificación

`crates/pypic-pic14/tests/text_parse.rs`: la conversión del **PIC**, la del **intérprete** de la IR y una **referencia independiente en Rust** (`pypic_frontend::text::parse`) coinciden en más de 60 textos: bordes de 16 bits (`65535`, `65536`, `6553`, `65539`, `0xFFFF`, `10000`), ceros a la izquierda, mayúsculas y minúsculas, con y sin prefijo, y texto inválido (vacío, `12a`, `-5`, `NUL`, `0x`, `G`, espacios). Además: los valores de ida y vuelta (cada borde pasa a texto y vuelve igual), el indicador que se reescribe siempre, `n` mayor que la capacidad, una variable local como indicador, la llamada como sentencia y los diagnósticos. `crates/pypic-pic14/tests/uart_examples.rs` prueba el convertidor completo por el puerto serie simulado. **No probado en hardware.**
