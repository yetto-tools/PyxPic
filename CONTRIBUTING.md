# Cómo contribuir a PyxPic

PyxPic compila programas con sintaxis Python a **código nativo** para PIC. Gracias por querer ayudar. Esta guía dice cómo se trabaja aquí y qué se espera de cada cambio.

## Antes de empezar

- Los **issues de GitHub son la fuente de verdad** de lo que falta (`docs/ISSUES_BACKLOG.md` solo guarda la historia). Si vas a hacer algo grande, comenta en el issue o ábrelo antes.
- Lee `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md` y `docs/principios-de-diseno.md`. Lo esencial:
  - **Python es solo sintaxis.** No hay intérprete, máquina virtual ni Python en el microcontrolador: todo se resuelve al compilar.
  - **Ensamblador de alto nivel:** conserva la flexibilidad y la precisión de ciclos del ensamblador. **Una sola forma por operación** y **ningún costo oculto**: si algo cuesta ciclos, RAM o flash, el costo se documenta y se mide.
  - **Todos los PIC, no solo los de 8 bits.** El dispositivo vive en un Device Pack (`devices/`), no en el código del compilador.
  - **El ensamblador usa la sintaxis oficial de PIC-AS** (XC8); no depender de GPASM ni de XC8 para compilar.

## Compilar y probar

```
cargo build --workspace
cargo test --workspace        # tests unitarios y diferenciales (intérprete de la IR contra el simulador del PIC)
cd vscode-pyxpic && npm test   # extensión de VS Code (necesita la CLI compilada)
```

Todo cambio debe dejar **`cargo test --workspace` en verde**, y pasar el formato y `clippy` (lo mismo que comprueba el CI):

```
cargo fmt --all                                         # rustfmt.toml: ancho 200, el codigo usa lineas anchas a proposito
cargo clippy --workspace --all-targets -- -D warnings
```

## Qué se espera de cada cambio

1. **Pruebas.** Un comportamiento nuevo trae pruebas, y si genera código, **pruebas diferenciales**: el intérprete de la IR y el simulador del núcleo deben dar el mismo resultado. Incluye los casos límite y los de error, no solo el feliz.
2. **Verificar contra el datasheet.** Registros, bits, tiempos y límites se leen del datasheet (mejor la imagen de la página que el texto extraído, que rompe las tablas) y el código cita la sección. Lo que es decisión de PyxPic y no del datasheet (por ejemplo, una tolerancia) se dice así.
3. **No afirmar lo que no se probó.** Distingue **simulador**, **Proteus** y **hardware real**. Una prueba en simulador no es una prueba en placa.
4. **Documentar.** Cada módulo del SDK tiene su `docs/sdk-*.md` con el contrato, los costos medidos, los diagnósticos y lo que **no** está. Un ejemplo ejecutable en `examples/`.
5. **Diagnósticos con ubicación**, código (`PYPnnnn`) y una solución sugerida.
6. **Un cambio, un propósito.** No mezcles cambios no relacionados en el mismo commit.

## Política por módulos

| Módulo | Dónde | Qué debe cumplir una contribución |
|---|---|---|
| Frontend (lexer, parser, verificador) | `crates/pyxpic-frontend` | Tests con el código de diagnóstico esperado; no aceptar sintaxis que el resto no soporte |
| IR e intérprete de referencia | `crates/pyxpic-ir` | `verify()` pasa; el intérprete es el oráculo de las pruebas diferenciales |
| Backend PIC14 y simulador | `crates/pyxpic-pic14` | Pruebas diferenciales; costos medidos en el simulador; el simulador modela el hardware según el datasheet |
| Device Packs | `devices/` y `crates/pyxpic-device` | `pyxpic device validate` pasa; datos con su fuente |
| Periféricos del SDK | `docs/sdk-*.md` | Sigue el patrón de ADC y UART: tipos del frontend, verificador, instrucción de IR, generación de código con nombres del Device Pack, modelo en el simulador, pruebas, documentación y ejemplo |
| CLI y proyecto (`pyxpic.toml`) | `crates/pyxpic-cli` | Salida estable (JSON versionado para herramientas); nunca sobrescribir archivos del usuario sin avisar |
| Ensamblador (PIC-AS) | `crates/pyxpic-pic14/src/asm.rs` | Comparar con PIC-AS cuando esté disponible; documentar las diferencias |
| Extensión de VS Code | `vscode-pyxpic/` | `npm test`; la lógica sin VS Code va en módulos propios para poder probarla |
| Documentación | `docs/`, `README.md` | Estado real: lo propuesto se marca como propuesta |

## Commits y revisión

- Mensajes de commit **en español**, con una descripción simple de qué cambia y por qué.
- Haz el commit cuando una pieza esté **terminada y con sus pruebas**, no a medias.
- Los cambios grandes de formato se hacen **solos**, en su propio commit.
- El trabajo entra en `main`; lo publicable vive en `release` y se etiqueta `vX.Y.Z` (ramas, CI y publicaciones: [`docs/release.md`](docs/release.md)).

## Licencia

PyxPic se distribuye bajo la **licencia MIT** (ver `LICENSE`). Al enviar una contribución aceptas que se publique bajo esa licencia.

Los **datasheets de Microchip** de `docs/` (los PDF) son de Microchip y **no** están bajo esta licencia: se incluyen como referencia de consulta.
