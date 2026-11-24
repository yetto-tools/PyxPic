# PyPIC — Especificación integral

Documentación de diseño consolidada a partir de las decisiones y propuestas discutidas. **Estado: planificación; no es software implementado.**

**Decisión no negociable:** PyPIC adopta la **sintaxis** de Python para escribir firmware compilado a código máquina nativo. **Nunca** se busca llevar un intérprete Python al microcontrolador.

## Documentos
- [Principio arquitectónico: NO intérprete Python](docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md)
- [Visión y alcance](docs/spec/01_VISION.md)
- [Compilador, IR y generación de código](docs/spec/02_COMPILADOR.md)
- [Device Packs y compatibilidad](docs/spec/03_DISPOSITIVOS.md)
- [VS Code, Wizard y perfiles](docs/spec/04_IDE_WIZARD.md)
- [SDK, librerías y programación visual](docs/spec/05_SDK.md)
- [Simulador y depuración](docs/spec/06_SIMULADOR_DEBUG.md)
- [I²C Scanner, terminal y herramientas de hardware](docs/spec/07_HERRAMIENTAS.md)
- [CLI, estructura de proyecto y contratos](docs/spec/08_CLI_FORMATOS.md)
- [Roadmap, pruebas y criterios de aceptación](docs/spec/09_ROADMAP.md)
- [Instrucciones para Codex](AGENTS.md)
- [Backlog de implementación](TASKS.md)

## Decisiones firmes
- Toolchain PyPIC Native independiente y predeterminado.
- Subconjunto Python compilado AOT, no intérprete.
- PIC16F887 primero, PIC12F625 después.
- Extensibilidad mediante IR, backends y Device Packs.
- Integración opcional con XC8/PIC-AS/SDCC mediante transpilación o ASM.
- VS Code, Wizard, pinout, simulador, debugger, I²C Scanner y terminal serial forman parte de la visión.

## Nota
Los prototipos UI y snippets anteriores son ilustrativos. Las APIs, versiones y capacidades deberán verificarse en implementación.

## Estructura del repositorio
```
crates/
  pypic-frontend/   lexer, parser, tipos y constantes (src/, tests/)
  pypic-ir/         IR tipada, lowering, verificador, intérprete de referencia (src/, tests/)
  pypic-device/     Device Packs: esquema, cargador y validador (src/, tests/)
  pypic-pic14/      backend PIC14: instrucciones, generador de código, desensamblador, Intel HEX, simulador del núcleo (src/, tests/)
  pypic-cli/        binario `pypic` (check, ir, build, asm, disasm, device validate)
devices/
  pic16f887/        Device Pack verificado contra DS40001291H
docs/
  spec/             especificación (docx, ZIP y módulos 00–09)
  frontend.md       diseño y semántica del frontend
  device-packs.md   esquema y validación de Device Packs
  backend-pic14.md  generador de código PIC14: alcance, límites y verificación
  sdk-gpio.md       acceso a pines (pypic.periph.gpio): API, reglas y ejemplo
  sdk-time.md       esperas exactas al ciclo (pypic.system.time): pause, wait, delay_us, delay_cycles
  principios-de-diseno.md  ensamblador de alto nivel: reglas e inventario de lo que falta
  catalogo-pic16f887.md    periféricos, protocolos y módulos del PIC16F887 (base de peripherals.json)
  sdk-modulos.md           árbol de módulos: pypic.periph / pypic.system / pypic.soft
  asm-pic14.md             ensamblador PIC14 (pypic asm): sintaxis, instrucciones y errores
  asm-inline.md            @asm_function: ensamblador dentro del lenguaje
  proteus-blink.md  cómo probar el blink de muestra en Proteus
  abi-pic14.md      ABI de PIC14 (convención de llamada, banco, ISR, pila)
  ISSUES_BACKLOG.md backlog propuesto como issues
examples/           programas PyPIC de ejemplo (.py); hex/ = .hex de muestra
AGENTS.md  TASKS.md
```
Las pruebas de cada crate viven en su propio `tests/` (convención de Cargo). Los backends, Device Packs, simulador y
extensión de VS Code se añadirán como nuevos crates/carpetas.

## Compilar y probar
```
cargo test
cargo run -p pypic-cli -- check examples/counter.py
cargo run -p pypic-cli -- ir examples/counter.py
cargo run -p pypic-cli -- build examples/counter.py -o counter.hex --listing
cargo run -p pypic-cli -- build examples/button_led.py -o button_led.hex   # el dispositivo lo declara el propio programa
cargo run -p pypic-cli -- build examples/blink.py -o blink.hex --timing      # informe de tiempos: pedido, ciclos y error
cargo run -p pypic-cli -- asm examples/asm/blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o blink_rd0.hex   # ensamblador PIC14 (sintaxis pic-as)
cargo run -p pypic-cli -- device validate devices/pic16f887
cargo run -p pypic-cli -- disasm archivo.hex    # inspecciona un .hex (programa y palabras de configuracion)
cargo run -p pypic-cli -- check examples/counter.py --device devices/pic16f887
```
