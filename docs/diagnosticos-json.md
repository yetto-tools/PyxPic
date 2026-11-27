# Diagnósticos en JSON: `pyxpic check --format json`

**Estado: implementado (#10).** Los diagnósticos del compilador en un formato **estable y versionado**, para el LSP, la extensión de VS Code y otras herramientas. La salida de texto de siempre no cambia.

```
pyxpic check programa.py --format json [--devices-dir DIR] [--device DIR] [--project pyxpic.toml] [--target perfil]
```

Solo vale con `check` (`build` e `ir` imprimen texto y rechazan `--format json` con código de salida 2). La salida estándar es **un solo documento JSON** y nada más (sin la línea `programa.py: ok`); `stderr` queda vacío. Código de salida: **0** si no hay errores (los avisos no impiden compilar) y **1** si hay algún error. Si ni siquiera se puede leer el archivo, el error sale como texto en `stderr` con código 2 (no hay análisis que informar).

## Esquema (versión 1)

```json
{
  "version": 1,
  "file": "prog.py",
  "ok": false,
  "summary": { "errors": 2, "warnings": 0 },
  "diagnostics": [
    {
      "code": "PYP2040",
      "severity": "error",
      "message": "el valor 300 no cabe en uint8 (rango 0..255)",
      "range": {
        "start": { "line": 10, "column": 16 },
        "end":   { "line": 10, "column": 19 }
      }
    }
  ]
}
```

| Campo | Tipo | Significado |
|---|---|---|
| `version` | entero | Versión del esquema. Cambia solo si se rompe a quien lo lee (se quita o cambia el significado de un campo); **agregar un campo no la sube**: ignora los campos que no conozcas |
| `file` | texto | El archivo, tal como se pasó en la línea de comandos |
| `ok` | bool | `true` si **no hay errores** (aunque haya avisos) |
| `summary.errors` / `summary.warnings` | entero | Cuántos diagnósticos hay de cada severidad |
| `diagnostics` | lista | Ordenada por posición en el archivo |
| `code` | texto | `PYP1xxx` sintaxis, `PYP2xxx` tipos y semántica, `PYP3xxx` avisos |
| `severity` | `"error"` \| `"warning"` | |
| `message` | texto | El mensaje, en español, sin la posición |
| `range.start` / `range.end` | `{line, column}` | **De 1 en adelante**; la columna cuenta **caracteres** (no bytes), igual que el texto. `end` es **exclusivo** (la posición justo después del último carácter) y puede estar en otra línea; si el diagnóstico es un punto, `end` es igual a `start` |

El orden de los campos de arriba es el de la salida y no hay que depender de él.

## Con errores de sintaxis

Con errores de sintaxis también salen los errores de tipos de lo que el parser sí leyó (#4): en el ejemplo, el `300` fuera de rango (línea 10) y el error de sintaxis de la línea 11 salen juntos. No hay avisos mientras haya errores de sintaxis.

## Cómo se garantiza que no cambia

`crates/pyxpic-cli/tests/snapshots/check_errors.json` y `check_warnings.json` son **instantáneas** del formato: `crates/pyxpic-cli/tests/cli.rs` compara la salida real con ellas **byte a byte**. Cambiar el formato a propósito exige actualizar la instantánea, y si rompe a quien lo lee, subir `version`. Otras pruebas comprueban un solo documento y nada en `stderr`, las posiciones (de 1 en adelante, el fin no está antes del inicio, el rango del `300`), un programa limpio (`ok: true` y lista vacía) y que `build` e `ir` rechazan `--format json`.

## Lo que no hace todavía

- **`build` e `ir`** no tienen formato JSON: sus diagnósticos van como texto.
- **Un código de error del compilador interno** (la IR no es válida, código de salida 70) y los errores de lectura salen como texto.
- **La extensión de VS Code** todavía lee el texto con una expresión regular (`language.diagnostics`); pasarla a este JSON le daría rangos exactos en lugar de un solo carácter. Está pendiente.
- **Sin sugerencias de arreglo** (quick fixes) ni diagnósticos relacionados: el esquema 1 no los tiene.

**No probado en hardware**: es la salida del compilador y no depende del dispositivo.
