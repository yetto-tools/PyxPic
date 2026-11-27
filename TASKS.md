# Backlog de implementación

Estado verificado (2026-11-24). Leyenda: **[x]** hecho y probado · **[~]** parcial, con su límite y el issue que lo cierra · **[ ]** sin empezar.
Hecho = hay código, pruebas que pasan (`cargo test --workspace`) y, donde aplica, verificación contra el datasheet o el simulador del núcleo.
Del conjunto marcado [x], en **hardware real** solo se confirmaron el blink, la EEPROM y la UART con un programador K150 y un montaje (`docs/programador-k150.md`, #16); lo demás se probó en el simulador, y el blink y el pulsador además en Proteus.
Los issues abiertos están en <https://github.com/yetto-tools/PyxPic/issues>.

## Fase 0 — Diseño
- [x] Gramática del subconjunto Python y sistema de tipos (`docs/frontend.md`)
- [x] IR y ABI PIC14 (`docs/abi-pic14.md`); [~] el overlay de memoria (#26)
- [x] Esquema y validación del Device Pack (`docs/device-packs.md`, `pyxpic device validate`)
- [x] Licencia (MIT), estructura monorepo y políticas de contribución (`LICENSE`, `CONTRIBUTING.md`, #18)

## Fase 1 — Compilador nativo
- [x] Parser y errores con ubicación (`crates/pyxpic-frontend`)
- [x] Tipado y constantes; [~] sin `u32`/`i32` en el backend (#32)
- [x] Instrucciones PIC14, codificador y desensamblador (las 35, ida y vuelta de las 16 384 palabras)
- [~] Bancos, saltos y llamadas: todo el dato en el banco 0 (#26); programas de hasta toda la flash con PCLATH (una función por página, +4 ciclos por llamada); sin enlazador entre módulos
- [x] Arreglos estáticos `uint8[N]` globales e indexado `buf[i]` (#8, primera entrega: `docs/sdk-arreglos.md`) · [ ] otros tipos de elemento, locales, inicializadores y `char[N]`/recepción de texto (#63).
- [x] Intel HEX y palabras de configuración por nombre (`pyxpic build --config`)
- [x] Blink y pulsador reales compilados desde `.py`, confirmados en Proteus; [x] blink, EEPROM y UART confirmados en hardware real con un K150 (#16, `docs/programador-k150.md`)
- [~] Ensamblador textual con un subconjunto de la sintaxis PIC-AS y ensamblador en línea (`@asm_function`); compatibilidad y directivas pendientes en #49–#52, #60, #66 y #67. `IDLOC0`–`IDLOC3` y su desensamblado están implementados (#68).
- [x] Intérprete de referencia de la IR y simulador del núcleo PIC14 (solo en el host, para pruebas diferenciales)

## Fase 2 — Hardware-aware
- [x] Device Pack del PIC16F887 verificado contra DS40001291H (registros, periféricos, pines, configuración); [~] solo el encapsulado DIP de 40 pines (#24)
- [~] Diagnósticos con código, severidad y ubicación en `pyxpic check`; **salida JSON para el editor** (#10) y pinout gráfico: pendientes
- SDK
  - [x] GPIO (`pyxpic.periph.gpio`) · [x] temporizadores 0/1/2 (`pyxpic.periph.timer`) · [x] registros y bits (`pyxpic.regs`) · [x] bits y acarreo (`pyxpic.bits`)
  - [x] esperas exactas al ciclo (`pyxpic.system.time`) · [x] tablas en flash (`Rom[u8]`) · [x] `*`, `//`, `%` y desplazamientos (rutinas nativas)
  - [x] ADC de 10 bits (`pyxpic.periph.adc`) · [x] UART asíncrona 8N1 y de 9 bits (`pyxpic.periph.uart`) · [ ] PWM/CCP, SPI, I²C, comparadores, EEPROM (resto de #45) · [ ] WDT, `sleep`/`nap`, reloj y energía (#46, #38) · [ ] protocolos por software (#47)

## Fase 3 — Herramientas
- [~] VS Code: extensión inicial para comprobar, compilar y abrir ejemplos (`vscode-pyxpic/README.md`); informe de recursos #65 y demás herramientas pendientes. [ ] Project Wizard.
- [~] Simulador: existe el simulador del núcleo para pruebas del backend; sin interfaz de usuario ni DAP
- [ ] Terminal serial y plotter
- [ ] I²C Scanner y explorador de registros

## Fase 4 — Extensibilidad
- [ ] PIC12F625 (#25)
- [ ] Backend ASM PIC-AS legible (#55) y C XC8/SDCC
- [ ] Adaptadores de toolchains y programadores
- [ ] Blockly y ecosistema de paquetes

## Criterio transversal de arquitectura
- [x] Cada backend produce firmware nativo autónomo, sin intérprete/VM Python, REPL ni bytecode Python embebido (el MCU solo ejecuta instrucciones del PIC; ver `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).
- [x] Separación host (parseo, IR, transpilación, simulación) vs dispositivo (instrucciones nativas).

## Cómo comprobar lo marcado como hecho
```
cargo test --workspace                                              # las diferenciales comparan el intérprete de la IR con el simulador del núcleo
cargo test -p pyxpic-pic14 --test runtime                            # *, //, %: todos los pares de u8/i8, bordes de 16 bits, interrupciones inyectadas, costos
cargo test -p pyxpic-pic14 --test asm                                # ensamblador pic-as; el blink en pic-as reproduce byte a byte el .hex probado en Proteus
cargo test -p pyxpic-device                                          # Device Pack del PIC16F887 contra sus propias reglas de coherencia
cargo run -p pyxpic-cli -- device validate devices/pic16f887
cargo run -p pyxpic-cli -- build examples/python/gpio/blink.py -o /tmp/blink.hex --timing --listing
cargo run -p pyxpic-cli -- asm examples/asm/gpio/blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o /tmp/b.hex   # compare con examples/hex/blink_rd0.hex
```
La comparación opcional con XC8 requiere configurar `PYXPIC_TEST_PIC_AS` y `PYXPIC_TEST_XC8_DFP` como se indica en `docs/asm-pic14.md`; cubre casos concretos, no toda la compatibilidad (#66, #67). Estos comandos no verifican el comportamiento en hardware real (solo hay confirmación en Proteus y la de `docs/programador-k150.md`, #16).
