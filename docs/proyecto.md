# El archivo de proyecto: `pypic.toml`

**Estado: implementado (primera entrega de #37).** Un archivo opcional que da al compilador el programa principal, la carpeta de Device Packs y uno o más **perfiles de destino**: dispositivo, reloj, palabras de configuración y salida, sin editar el fuente. Sin `pypic.toml` todo sigue funcionando como antes.

```toml
[project]
name = "parpadeo"                  # opcional
main = "main.py"                   # programa por defecto: `pypic build` sin archivo
devices_dir = "../../devices"      # carpeta de Device Packs (relativa a este archivo)
default_target = "interna"         # perfil por defecto (o el único, si solo hay uno)

[target.interna]                   # un perfil por destino
device = "PIC16F887"
clock = "Internal[4 * MHz]"        # la misma declaración que en el fuente (docs/sdk-reloj.md)
output = "build/parpadeo_interna.hex"

[target.cristal]
device = "PIC16F887"
clock = "Crystal[20 * MHz]"
config = { WDTE = "OFF" }          # palabras de configuración por nombre, como --config
output = "build/parpadeo_cristal.hex"
```

```
pypic build                       # perfil por defecto
pypic build --target cristal      # el mismo programa para otro destino
pypic build --project otro/pypic.toml
pypic check | pypic ir            # también leen el proyecto
```

Se busca el `pypic.toml` **más cercano** al programa (en su carpeta o en las de arriba) o a la carpeta actual si no se da programa. Las rutas del archivo son relativas **a él**. Un ejemplo completo está en `examples/proyecto/`.

## Quién manda

1. **La línea de comandos** manda sobre el perfil: `--config WDTE=OFF` gana al `config = { WDTE = "ON" }` del perfil (sin "se indica dos veces"), y `-o` gana a `output`.
2. **El fuente manda sobre el perfil** en lo que declara:
   - **Dispositivo:** si el programa tiene `from pypic.device import PIC16F887` y el perfil pide **otro**, es un **error** con los dos nombres y el archivo (`el programa declara 'from pypic.device import PIC16F887' y el perfil 'otra' de pypic.toml pide PIC16F886`). Si pide el mismo, no pasa nada.
   - **Reloj:** si el programa declara `CLOCK: ...` o `CLOCK_HZ`, el perfil **no lo cambia**; si el del perfil es distinto, se **avisa** (`manda el programa`).
3. **El perfil completa lo que el fuente no dice:** un programa **sin** `from pypic.device import ...` toma el dispositivo del perfil (esa línea pasa a ser opcional), y sin reloj toma el del perfil. El dispositivo sigue siendo obligatorio en `pypic build`: si no está en ningún lado, el error dice dónde ponerlo (el reloj, si falta, toma el por defecto del dispositivo con un aviso, como siempre).
4. `CLOCK` fija el oscilador (`FOSC`), así que un `config = { FOSC = ... }` en el perfil con un reloj declarado es el mismo error que con `--config FOSC` (docs/sdk-reloj.md).

## Validación

El archivo se valida **entero** antes de compilar, con el nombre del archivo y las claves válidas en el mensaje:

| Error | Ejemplo del mensaje |
|---|---|
| Sección desconocida | `la seccion 'proyecto' no existe (admite: project, target)` |
| Clave desconocida | `[target.a] la clave 'devce' no existe (admite: device, clock, config, output)` |
| Tipo equivocado | `[target.a] 'device' es un texto no vacio`, `'config' es una tabla` |
| `default_target` inexistente | `no hay [target.x] (hay: a, b)` |
| Varios perfiles sin elegir | `hay varios perfiles (a, b) y ninguno por defecto: elige uno con --target o fija default_target` |
| Perfil inexistente | `no hay un perfil 'zzz'; hay: a, b` |
| `--target` sin proyecto | `--target necesita un pypic.toml` |
| Reloj inválido en el perfil | Las mismas reglas del fuente: `[target.a] clock = "Internal[20 * MHz]" no es valido` con el `PYP2194` |

Los errores de lectura salen con código de salida 2; los de contenido (dispositivo contrario, reloj inválido) con 1. La carpeta de `output` se **crea** si falta.

## Lo que no está (todavía)

- **`pypic init`, `pypic project check/set/migrate`** y el asistente de la extensión (ver `docs/spec/04_IDE_WIZARD.md`): el archivo ya es lo que esas órdenes escribirán.
- **`targets/*.toml` separados, lockfile, varios programas por proyecto, perfiles `Debug`/`Release`** y una opción global por usuario.
- **`pypic migrate --dry-run`** para comparar el programa contra otro MCU.
- **Compilar contra todos los perfiles** de una vez (`--target all`).
- **Programador y puerto de grabación** (`pypic flash`): ver `docs/spec/07_HERRAMIENTAS.md`.

## Verificación

`crates/pypic-cli/src/project.rs` (pruebas unitarias del análisis: un proyecto completo con rutas relativas, los errores con sus claves válidas y la elección de perfil) y `crates/pypic-cli/tests/cli.rs`: un proyecto que compila **sin argumentos** con dispositivo, reloj (`FOSC = HS`, 100 ms = 500 000 ciclos a 20 MHz), configuración (`WDTE`) y salida tomados del perfil y sin `from pypic.device import ...` en el fuente; el conflicto de dispositivo y el aviso de reloj; `--target`, `default_target` y `--config` sobre el perfil; los errores del archivo; y el ejemplo `examples/proyecto/` con sus dos destinos. **No probado en hardware.**
