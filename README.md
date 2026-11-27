# PyxPic Compiler

> **Cambio de nombre:** el proyecto se llamaba **PyPIC**; ahora es **PyxPic** para no confundirse con otros de nombre igual o parecido. Se renombró todo y **no hay compatibilidad** con el nombre anterior. Para migrar un programa: `from pypic...` pasa a `from pyxpic...`, `pypic.toml` a `pyxpic.toml`, el comando `pypic` a `pyxpic`, `PYPIC_DEVICES` a `PYXPIC_DEVICES` y los ajustes de VS Code `pypic.*` a `pyxpic.*`. Los códigos de diagnóstico (`PYP...`, `PYB...`) no cambian. Los documentos `.docx` y `.zip` de `docs/spec` conservan el nombre antiguo.

Compilador de firmware para microcontroladores PIC con la **sintaxis de Python** y resultado en **código máquina nativo**: PyxPic es un *ensamblador de alto nivel* (`docs/principios-de-diseno.md`).
**Nunca** se lleva un intérprete Python al microcontrolador (`docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).

## Estado

**En desarrollo activo. Funciona para el PIC16F887 dentro de los límites de abajo; no es un producto completo.** El detalle verificado, componente por componente, está en [`TASKS.md`](TASKS.md).

| | |
|---|---|
| **Funciona** (512 pruebas, muchas diferenciales entre el intérprete de referencia y un simulador del núcleo) | frontend con tipos y diagnósticos con ubicación · IR y backend PIC14 · las 35 instrucciones y un ensamblador con la **sintaxis de pic-as** (macros, `PSECT`, `CONFIG`) · Intel HEX · Device Pack del PIC16F887 · GPIO, temporizadores 0/1/2, ADC de 10 bits, UART 8N1 y de 9 bits, texto y f-strings en la UART, arreglos `uint8[N]`, registros y bits, acarreo, esperas exactas al ciclo, tablas en flash, `*` `//` `%` y desplazamientos · interrupciones · `@asm_function` |
| **Confirmado en Proteus y en hardware** | `blink` y `button_led` compilados desde `.py` en Proteus (`docs/proteus-blink.md`). En **hardware real** (un PIC16F887, un programador K150 y un montaje; sin medir voltajes ni la exactitud al ciclo): el parpadeo, la EEPROM y la UART (`docs/programador-k150.md`) |
| **Límites conocidos** | solo el PIC16F887 (#25) · sin `u32`/`i32` (#32) · todo el dato en el banco 0 (#26) y una función no puede pasar de 2048 palabras · PWM solo de una salida (sin captura, comparación ni puentes), SPI e I²C solo en modo maestro y sin probar en hardware, sin pestillo SR (resto de #45) · extensión VS Code inicial, sin depurador ni terminal serie |
| **Sin empezar** | Wizard, DAP, I²C Scanner y la Fase 4 (otros PIC, backends C/ASM, Blockly) |

## Documentos
- Especificación: [principio NO intérprete](docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md) · [visión](docs/spec/01_VISION.md) · [compilador e IR](docs/spec/02_COMPILADOR.md) · [dispositivos](docs/spec/03_DISPOSITIVOS.md) · [IDE y Wizard](docs/spec/04_IDE_WIZARD.md) · [SDK](docs/spec/05_SDK.md) · [simulador y depuración](docs/spec/06_SIMULADOR_DEBUG.md) · [herramientas](docs/spec/07_HERRAMIENTAS.md) · [CLI y formatos](docs/spec/08_CLI_FORMATOS.md) · [roadmap](docs/spec/09_ROADMAP.md). **Es la visión de diseño**; lo implementado está en `TASKS.md`.
- [Instrucciones para Codex](AGENTS.md) · [Backlog de implementación](TASKS.md) · [Historia del backlog](docs/ISSUES_BACKLOG.md)

## Decisiones firmes
- Toolchain PyxPic Native independiente y predeterminado; subconjunto de Python compilado AOT, no intérprete.
- PIC16F887 primero, PIC12F625 después; extensibilidad mediante IR, backends y Device Packs.
- Una sola forma por operación, sin costos ocultos y esperas exactas al ciclo.
- Integración opcional con XC8/PIC-AS/SDCC mediante transpilación o ASM (hoy: el ensamblador acepta la sintaxis de PIC-AS).
- La [extensión inicial de VS Code](vscode-pyxpic/README.md) permite comprobar y compilar con la CLI y abrir ejemplos; Wizard, pinout, simulador visual, debugger, I²C Scanner y terminal serial siguen pendientes.

## Estructura del repositorio
```
crates/
  pyxpic-frontend/   lexer, parser, tipos, constantes y SDK en el verificador (src/, tests/)
  pyxpic-ir/         IR tipada, lowering, verificador, intérprete de referencia
  pyxpic-device/     Device Packs: esquema, cargador y validador
  pyxpic-pic14/      backend PIC14: generador, ensamblador (pic-as), desensamblador, Intel HEX, simulador del núcleo
  pyxpic-cli/        binario `pyxpic` (check, ir, build, asm, disasm, device validate)
vscode-pyxpic/       extensión inicial de VS Code
devices/pic16f887/  Device Pack verificado contra DS40001291H
examples/           programas de ejemplo (.py y .s); hex/ = .hex de muestra
docs/               diseño, SDK y guías (ver abajo)
AGENTS.md  TASKS.md
```
Guías del SDK y del compilador (`docs/`): [frontend](docs/frontend.md) · [Device Packs](docs/device-packs.md) · [backend PIC14](docs/backend-pic14.md) · [ABI](docs/abi-pic14.md) · [principios de diseño](docs/principios-de-diseno.md) ·
[módulos del SDK](docs/sdk-modulos.md) · [GPIO](docs/sdk-gpio.md) · [tiempo](docs/sdk-time.md) · [temporizadores](docs/sdk-timer.md) · [registros](docs/sdk-regs.md) · [bits y acarreo](docs/sdk-bits.md) ·
[tablas](docs/sdk-tablas.md) · [runtime: `*` `//` `%`](docs/sdk-runtime.md) · [ensamblador](docs/asm-pic14.md) · [ensamblador en línea](docs/asm-inline.md) · [catálogo del PIC16F887](docs/catalogo-pic16f887.md) · [Proteus](docs/proteus-blink.md) · [grabar con un K150](docs/programador-k150.md).

Las pruebas de cada crate viven en su propio `tests/` (convención de Cargo).

## Compilar el proyecto

Para instrucciones detalladas, consulta la [guía de compilación de PyxPic](docs/compilar-pyxpic.md).

Requisitos: [Rust y Cargo](https://www.rust-lang.org/tools/install) instalados. La primera compilación necesita acceso a las dependencias de Cargo. Abre una terminal en la raíz del repositorio, donde está `Cargo.toml`:

```powershell
cargo build --workspace
cargo test --workspace
```

El primer comando compila todos los paquetes Rust; el segundo ejecuta las pruebas del workspace. El ejecutable de la CLI queda en `target/debug/pyxpic.exe` en Windows y en `target/debug/pyxpic` en Linux o macOS. Para una versión optimizada, ejecuta `cargo build --workspace --release`; el ejecutable queda en `target/release/`.

La extensión de VS Code es un componente aparte: requiere VS Code de escritorio y Node.js para sus pruebas; consulta [su guía](vscode-pyxpic/README.md) para configurarla y ejecutarla. Desde la raíz, tras compilar la CLI, sus pruebas se ejecutan con:

```powershell
node --test vscode-pyxpic/test/*.test.js
```

XC8 y PIC-AS son opcionales: solo se necesitan para las pruebas diferenciales con el ensamblador de Microchip. Si no se configuran, esas comparaciones se omiten; ver [comparación con PIC-AS](docs/asm-pic14.md#comparación-con-xc8-pic-as). No se necesita GPASM para compilar PyxPic.

## Usar la CLI

Desde la raíz del repositorio, `cargo run` compila la CLI si hace falta y ejecuta el comando solicitado. Por ejemplo:

```powershell
cargo run -p pyxpic-cli -- check examples/python/gpio/counter.py
cargo run -p pyxpic-cli -- ir examples/python/gpio/counter.py
cargo run -p pyxpic-cli -- build examples/python/gpio/counter.py -o counter.hex --listing
cargo run -p pyxpic-cli -- build examples/python/gpio/button_led.py -o button_led.hex      # el dispositivo lo declara el propio programa
cargo run -p pyxpic-cli -- build examples/python/gpio/blink.py -o blink.hex --timing         # informe de tiempos: pedido, ciclos y error
cargo run -p pyxpic-cli -- build examples/python/gpio/blink.py --config WDTE=ON,FOSC=XT      # palabras de configuración por nombre
cargo run -p pyxpic-cli -- asm examples/asm/gpio/blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o blink_rd0.hex   # sintaxis pic-as
cargo run -p pyxpic-cli -- device validate devices/pic16f887
cargo run -p pyxpic-cli -- disasm archivo.hex     # inspecciona un .hex (programa y palabras de configuración)
```

## Licencia

PyxPic se distribuye bajo la **licencia MIT** (ver [`LICENSE`](LICENSE)). Para contribuir, lee [`CONTRIBUTING.md`](CONTRIBUTING.md). Los datasheets de Microchip de `docs/` son de Microchip y no están bajo esta licencia.
