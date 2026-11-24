# PyPIC

Compilador de firmware para microcontroladores PIC con la **sintaxis de Python** y resultado en **código máquina nativo**: PyPIC es un *ensamblador de alto nivel* (`docs/principios-de-diseno.md`).
**Nunca** se lleva un intérprete Python al microcontrolador (`docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).

## Estado

**En desarrollo activo. Funciona para el PIC16F887 dentro de los límites de abajo; no es un producto completo.** El detalle verificado, componente por componente, está en [`TASKS.md`](TASKS.md).

| | |
|---|---|
| **Funciona** (428 pruebas, muchas diferenciales entre el intérprete de referencia y un simulador del núcleo) | frontend con tipos y diagnósticos con ubicación · IR y backend PIC14 · las 35 instrucciones y un ensamblador con la **sintaxis de pic-as** (macros, `PSECT`, `CONFIG`) · Intel HEX · Device Pack del PIC16F887 · GPIO, temporizadores 0/1/2, ADC de 10 bits, registros y bits, acarreo, esperas exactas al ciclo, tablas en flash, `*` `//` `%` y desplazamientos · interrupciones · `@asm_function` |
| **Confirmado en Proteus** | `blink` y `button_led` compilados desde `.py` (`docs/proteus-blink.md`). **Falta confirmarlo en hardware real** (#16) |
| **Límites conocidos** | solo el PIC16F887 (#25) · sin `u32`/`i32` (#32) · todo el dato en el banco 0 (#26) y una función no puede pasar de 2048 palabras · sin PWM, UART, SPI, I²C ni EEPROM (resto de #45) · sin extensión de VS Code, depurador ni terminal serie |
| **Sin empezar** | herramientas de la Fase 3 (VS Code, Wizard, DAP, I²C Scanner) y la Fase 4 (otros PIC, backends C/ASM, Blockly) |

## Documentos
- Especificación: [principio NO intérprete](docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md) · [visión](docs/spec/01_VISION.md) · [compilador e IR](docs/spec/02_COMPILADOR.md) · [dispositivos](docs/spec/03_DISPOSITIVOS.md) · [IDE y Wizard](docs/spec/04_IDE_WIZARD.md) · [SDK](docs/spec/05_SDK.md) · [simulador y depuración](docs/spec/06_SIMULADOR_DEBUG.md) · [herramientas](docs/spec/07_HERRAMIENTAS.md) · [CLI y formatos](docs/spec/08_CLI_FORMATOS.md) · [roadmap](docs/spec/09_ROADMAP.md). **Es la visión de diseño**; lo implementado está en `TASKS.md`.
- [Instrucciones para Codex](AGENTS.md) · [Backlog de implementación](TASKS.md) · [Historia del backlog](docs/ISSUES_BACKLOG.md)

## Decisiones firmes
- Toolchain PyPIC Native independiente y predeterminado; subconjunto de Python compilado AOT, no intérprete.
- PIC16F887 primero, PIC12F625 después; extensibilidad mediante IR, backends y Device Packs.
- Una sola forma por operación, sin costos ocultos y esperas exactas al ciclo.
- Integración opcional con XC8/PIC-AS/SDCC mediante transpilación o ASM (hoy: el ensamblador acepta la sintaxis de PIC-AS).
- VS Code, Wizard, pinout, simulador, debugger, I²C Scanner y terminal serial forman parte de la visión (todavía no están).

## Estructura del repositorio
```
crates/
  pypic-frontend/   lexer, parser, tipos, constantes y SDK en el verificador (src/, tests/)
  pypic-ir/         IR tipada, lowering, verificador, intérprete de referencia
  pypic-device/     Device Packs: esquema, cargador y validador
  pypic-pic14/      backend PIC14: generador, ensamblador (pic-as), desensamblador, Intel HEX, simulador del núcleo
  pypic-cli/        binario `pypic` (check, ir, build, asm, disasm, device validate)
devices/pic16f887/  Device Pack verificado contra DS40001291H
examples/           programas de ejemplo (.py y .s); hex/ = .hex de muestra
docs/               diseño, SDK y guías (ver abajo)
AGENTS.md  TASKS.md
```
Guías del SDK y del compilador (`docs/`): [frontend](docs/frontend.md) · [Device Packs](docs/device-packs.md) · [backend PIC14](docs/backend-pic14.md) · [ABI](docs/abi-pic14.md) · [principios de diseño](docs/principios-de-diseno.md) ·
[módulos del SDK](docs/sdk-modulos.md) · [GPIO](docs/sdk-gpio.md) · [tiempo](docs/sdk-time.md) · [temporizadores](docs/sdk-timer.md) · [registros](docs/sdk-regs.md) · [bits y acarreo](docs/sdk-bits.md) ·
[tablas](docs/sdk-tablas.md) · [runtime: `*` `//` `%`](docs/sdk-runtime.md) · [ensamblador](docs/asm-pic14.md) · [ensamblador en línea](docs/asm-inline.md) · [catálogo del PIC16F887](docs/catalogo-pic16f887.md) · [Proteus](docs/proteus-blink.md).

Las pruebas de cada crate viven en su propio `tests/` (convención de Cargo).

## Compilar y probar
```
cargo test
cargo run -p pypic-cli -- check examples/counter.py
cargo run -p pypic-cli -- ir examples/counter.py
cargo run -p pypic-cli -- build examples/counter.py -o counter.hex --listing
cargo run -p pypic-cli -- build examples/button_led.py -o button_led.hex      # el dispositivo lo declara el propio programa
cargo run -p pypic-cli -- build examples/blink.py -o blink.hex --timing         # informe de tiempos: pedido, ciclos y error
cargo run -p pypic-cli -- build examples/blink.py --config WDTE=ON,FOSC=XT      # palabras de configuración por nombre
cargo run -p pypic-cli -- asm examples/asm/blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o blink_rd0.hex   # sintaxis pic-as
cargo run -p pypic-cli -- device validate devices/pic16f887
cargo run -p pypic-cli -- disasm archivo.hex     # inspecciona un .hex (programa y palabras de configuración)
```
