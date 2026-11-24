# Backlog de implementación

Estado verificado (2026-11-24). Leyenda: **[x]** hecho y probado · **[~]** parcial, con su límite y el issue que lo cierra · **[ ]** sin empezar.
Hecho = hay código, pruebas que pasan (`cargo test --workspace`) y, donde aplica, verificación contra el datasheet o el simulador del núcleo.
Nada de lo marcado [x] se ha probado todavía en **hardware real**: el blink y el pulsador se confirmaron en Proteus (#16).
Los issues abiertos están en <https://github.com/yetto-tools/PyPic/issues>.

## Fase 0 — Diseño
- [x] Gramática del subconjunto Python y sistema de tipos (`docs/frontend.md`)
- [x] IR y ABI PIC14 (`docs/abi-pic14.md`); [~] el overlay de memoria (#26)
- [x] Esquema y validación del Device Pack (`docs/device-packs.md`, `pypic device validate`)
- [~] Licencia, estructura monorepo y políticas de contribución: monorepo hecho; **licencia y CONTRIBUTING pendientes** (#18)

## Fase 1 — Compilador nativo
- [x] Parser y errores con ubicación (`crates/pypic-frontend`)
- [x] Tipado y constantes; [~] sin `u32`/`i32` en el backend (#32)
- [x] Instrucciones PIC14, codificador y desensamblador (las 35, ida y vuelta de las 16 384 palabras)
- [~] Bancos, saltos y llamadas: todo el dato en el banco 0 (#26); programas de hasta toda la flash con PCLATH (una función por página, +4 ciclos por llamada); sin enlazador entre módulos
- [x] Intel HEX y palabras de configuración por nombre (`pypic build --config`)
- [x] Blink y pulsador reales compilados desde `.py`, confirmados en Proteus; [ ] confirmación en hardware (#16)
- [~] Ensamblador textual con un subconjunto de la sintaxis PIC-AS y ensamblador en línea (`@asm_function`); compatibilidad y directivas pendientes en #49–#52, #60, #66 y #67. `IDLOC0`–`IDLOC3` y su desensamblado están implementados (#68).
- [x] Intérprete de referencia de la IR y simulador del núcleo PIC14 (solo en el host, para pruebas diferenciales)

## Fase 2 — Hardware-aware
- [x] Device Pack del PIC16F887 verificado contra DS40001291H (registros, periféricos, pines, configuración); [~] solo el encapsulado DIP de 40 pines (#24)
- [~] Diagnósticos con código, severidad y ubicación en `pypic check`; **salida JSON para el editor** (#10) y pinout gráfico: pendientes
- SDK
  - [x] GPIO (`pypic.periph.gpio`) · [x] temporizadores 0/1/2 (`pypic.periph.timer`) · [x] registros y bits (`pypic.regs`) · [x] bits y acarreo (`pypic.bits`)
  - [x] esperas exactas al ciclo (`pypic.system.time`) · [x] tablas en flash (`Rom[u8]`) · [x] `*`, `//`, `%` y desplazamientos (rutinas nativas)
  - [x] ADC de 10 bits (`pypic.periph.adc`) · [x] UART asíncrona 8N1 y de 9 bits (`pypic.periph.uart`) · [ ] PWM/CCP, SPI, I²C, comparadores, EEPROM (resto de #45) · [ ] WDT, `sleep`/`nap`, reloj y energía (#46, #38) · [ ] protocolos por software (#47)

## Fase 3 — Herramientas
- [ ] VS Code y Project Wizard
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
cargo test -p pypic-pic14 --test runtime                            # *, //, %: todos los pares de u8/i8, bordes de 16 bits, interrupciones inyectadas, costos
cargo test -p pypic-pic14 --test asm                                # ensamblador pic-as; el blink en pic-as reproduce byte a byte el .hex probado en Proteus
cargo test -p pypic-device                                          # Device Pack del PIC16F887 contra sus propias reglas de coherencia
cargo run -p pypic-cli -- device validate devices/pic16f887
cargo run -p pypic-cli -- build examples/blink.py -o /tmp/blink.hex --timing --listing
cargo run -p pypic-cli -- asm examples/asm/blink_rd0.s -Wl,-presetVec=0h,-pintentry=4h -o /tmp/b.hex   # compare con examples/hex/blink_rd0.hex
```
La comparación opcional con XC8 requiere configurar `PYPIC_TEST_PIC_AS` y `PYPIC_TEST_XC8_DFP` como se indica en `docs/asm-pic14.md`; cubre casos concretos, no toda la compatibilidad (#66, #67). Estos comandos no verifican el comportamiento en hardware real (solo hay confirmación en Proteus, #16).
