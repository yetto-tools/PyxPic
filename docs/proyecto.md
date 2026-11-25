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

## Crear y revisar: `pypic init` y `pypic project check`

```
pypic init [carpeta] [--name NOMBRE] [--device PIC16F887] [--clock "Crystal[20 * MHz]"] [--devices-dir DIR]
pypic project check [--project pypic.toml] [--target perfil] [--format text|json] [--devices-dir DIR]
```

**`pypic init`** crea `pypic.toml` y un `main.py` de partida (parpadeo de RB0) en la carpeta (por defecto la actual). El dispositivo por defecto es `PIC16F887` (acepta `16f887` y `PIC16F887`) y el reloj, el que el dispositivo tiene tras el reset (`Internal[4 * MHz]`); con `--devices-dir` el proyecto recuerda dónde están los Device Packs (`devices_dir`). **Antes de escribir nada** comprueba que el dispositivo existe, que el reloj es válido y que el programa de partida es válido para ese destino: si algo falla, **no se crea nada**. **Nunca sobrescribe**: si ya existe `pypic.toml` o `main.py`, termina con error diciendo cuáles y no toca ni el otro archivo.

**`pypic project check`** revisa el proyecto **sin modificar ningún archivo** (ni siquiera crea la carpeta de salida). Para cada perfil (o solo `--target`) comprueba: el dispositivo y su Device Pack (y que no contradiga al del programa), el reloj con las reglas de `docs/sdk-reloj.md`, cada palabra de configuración por nombre y valor, que el programa exista y su análisis completo para ese destino (errores y avisos con archivo, línea y código). Sale con **0** si todos los perfiles están bien, **1** si alguno tiene errores y **2** si no encuentra o no puede leer el proyecto.

El formato `--format json` es **versionado** (`"version": 1`) para herramientas como la extensión de VS Code:

```json
{ "version": 1, "file": "...pypic.toml", "name": "p", "main": "...main.py", "devices_dir": "...", "default_target": null,
  "ok": false,
  "targets": [
    { "name": "ok", "status": "ok", "device": "PIC16F887", "config": { "WDTE": "ON" }, "output": null,
      "clock": { "declaration": "Crystal[8 * MHz]", "description": "Crystal 8 MHz (FOSC = HS)" },
      "errors": [], "warnings": [] },
    { "name": "mal", "status": "error", "device": "PIC16F887", "clock": { "declaration": "Crystal[24 * MHz]", "description": null },
      "config": {}, "output": null, "errors": ["[target.mal] clock = \"Crystal[24 * MHz]\": ... PYP2194 ..."], "warnings": [] } ] }
```

Los Device Packs se buscan en `devices_dir` del proyecto, en `$PYPIC_DEVICES` y en `./devices`: un proyecto creado **fuera** de la carpeta del compilador necesita `--devices-dir` (o la variable de entorno) para encontrarlos.

## Reconfigurar: `pypic project set`

```
pypic project set [--target perfil] [--create] [--dry-run] [--unset CLAVE]... [CLAVE=VALOR]...

pypic project set clock="Crystal[20 * MHz]" config.WDTE=ON       # el perfil por defecto (o el único)
pypic project set --target cristal device=PIC16F887               # un perfil concreto
pypic project set --target nueva --create device=PIC16F887 clock="Internal[8 * MHz]"
pypic project set --unset config.WDTE --unset output
pypic project set project.default_target=cristal project.name=termometro
pypic project set --dry-run clock="Crystal[8 * MHz]"              # solo muestra los cambios
```

Claves de perfil: `device`, `clock`, `output` y `config.AJUSTE` (un entero queda como entero: `config.BOREN=1`). Claves de proyecto (no necesitan perfil): `project.name`, `project.main`, `project.devices_dir`, `project.default_target`. `--create` crea el perfil si no existe y `--unset` quita una clave (al quitar la última `config.*` desaparece la tabla `config`).

- **Edita, no reescribe:** el archivo se modifica con `toml_edit`: los **comentarios** (de línea y de final de línea), el orden y los espacios **se conservan**; solo cambia lo pedido.
- **Valida el resultado entero antes de escribir:** estructura del archivo y el perfil cambiado con las mismas reglas de `project check` (dispositivo y pack, **el programa no declara otro dispositivo**, reloj, configuración, análisis del programa). Si algo queda inválido, **no se escribe nada**, ni la copia, y sale con código 1 explicando qué.
- **Copia de seguridad:** antes de escribir guarda la versión anterior en `pypic.toml.bak` (se reemplaza en cada cambio). Un cambio que no cambia nada (`clock` al mismo valor) no escribe ni crea copia.
- **`--dry-run`** muestra los cambios (`placa.clock: Internal[4 * MHz] -> Crystal[20 * MHz]`) y los avisos, y no escribe nada.
- Códigos de salida: 0 bien, 1 el cambio dejaría el archivo o el perfil inválido, 2 uso incorrecto (clave desconocida, falta `CLAVE=VALOR`, varios perfiles sin `--target`, perfil inexistente sin `--create`) o error de lectura/escritura.

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

- **`pypic project migrate --dry-run`** (comparar el programa contra otro MCU, con propuesta de remapeo de pines) y las pantallas del asistente de la extensión (ver `docs/spec/04_IDE_WIZARD.md`): `init`, `project check` y `project set` ya son la base sobre la que se apoyarán. **Pedir confirmación interactiva** antes de escribir tampoco está: el equivalente es `--dry-run` más la copia `.bak`.
- **`targets/*.toml` separados, lockfile, varios programas por proyecto, perfiles `Debug`/`Release`** y una opción global por usuario.
- **`pypic migrate --dry-run`** para comparar el programa contra otro MCU.
- **Compilar contra todos los perfiles** de una vez (`--target all`).
- **Programador y puerto de grabación** (`pypic flash`): ver `docs/spec/07_HERRAMIENTAS.md`.

## Verificación

`crates/pypic-cli/tests/cli.rs` prueba también `project set` (solo cambian las claves pedidas y se conservan los comentarios, `--unset`, `--dry-run`, los cambios inválidos que no escriben nada, perfiles nuevos, claves de proyecto y los errores de uso), y `init` y `project check`: un proyecto recién creado que se revisa y compila sin editar nada, el alias del dispositivo y el reloj, que no se crea nada si algo no es válido ni se sobrescribe un archivo existente, todos los problemas por perfil (dispositivo contrario, reloj imposible, configuración inexistente o con valor inválido, programa ausente o con errores) sin modificar archivos, y el JSON versionado. `crates/pypic-cli/src/project.rs` (pruebas unitarias del análisis: un proyecto completo con rutas relativas, los errores con sus claves válidas y la elección de perfil) y `crates/pypic-cli/tests/cli.rs`: un proyecto que compila **sin argumentos** con dispositivo, reloj (`FOSC = HS`, 100 ms = 500 000 ciclos a 20 MHz), configuración (`WDTE`) y salida tomados del perfil y sin `from pypic.device import ...` en el fuente; el conflicto de dispositivo y el aviso de reloj; `--target`, `default_target` y `--config` sobre el perfil; los errores del archivo; y el ejemplo `examples/proyecto/` con sus dos destinos. **No probado en hardware.**
