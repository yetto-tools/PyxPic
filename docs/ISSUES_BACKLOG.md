# Backlog propuesto como issues

Borrador para crear en GitHub (`yetto-tools/PyPic`). Labels existentes: `bug`, `enhancement`, `documentation`,
`question`, `good first issue`, `help wanted`. Sugerido anadir: `compiler`, `ir`, `backend`, `device-pack`, `testing`, `design-decision`.

## Hecho (cerrar al crear el primer commit)
- Frontend: lexer, parser, verificador de tipos y constantes (`crates/pypic-frontend`, `docs/frontend.md`).
- IR tipada + lowering + verificador + `pypic ir` (`crates/pypic-ir`).

## Decisiones de diseno (`question`, `design-decision`) — RESUELTAS (#1, #2, #3; ver docs/frontend.md)
1. **Politica de tipos anchos en MCU pequenos.** Hoy `--max-int-bits 8` rechaza `u16`. Un PIC16 puede emular 16/32 bits por
   software: decidir si el Device Pack lo permite con advertencia de coste.
2. **Revisar semantica fijada:** `//` y `%` truncan hacia cero; condiciones solo `bool`; conversiones siempre explicitas;
   overflow constante = error.
3. **Reentrancia con memoria estatica.** Locales/parametros son slots estaticos: una funcion llamada desde `main` y desde
   la `@interrupt` comparte sus slots. Definir regla (prohibirlo, duplicar o proteger) antes del ABI.

## Compilador / IR (`enhancement`, `compiler`/`ir`)
4. **Mostrar errores de tipos aunque haya errores de sintaxis** (hoy el verificador no corre si el parser fallo).
5. **Advertencias:** codigo inalcanzable, variables sin usar, parametros sin usar.
6. **Optimizaciones de IR:** propagacion de copias/constantes, eliminacion de codigo muerto y bloques vacios (hoy `bb2: jump bb3`).
7. **Interprete de IR de referencia (solo para tests en el host)** para validar semantica (p. ej. el bucle `range` sin wrap-around).
   No es un interprete Python ni va al MCU; ver `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`.
8. **Arreglos estaticos** (`buf: u8[16]`) e indexado.
9. **Resolucion de modulos del SDK** (`machine`, `gpio`, `time`...) con firmas por Device Pack.
10. **Salida JSON de diagnosticos** (`pypic check --format json`) para LSP y VS Code.
11. **Slots de `Const` locales**: no reservar memoria para constantes locales.

## Backend PIC14 / Device Packs (`enhancement`, `backend`, `device-pack`)
12. **Schema y validacion de Device Pack** (Fase 0) y construir `TargetProfile` desde el pack (no solo `max_int_bits`).
13. **ABI PIC14:** registros temporales, W/STATUS/PCLATH, bancos, solapamiento de locales por grafo de llamadas, vectores.
14. **Encoder de instrucciones PIC14 + ensamblador** (verificar contra datasheet oficial del PIC16F887).
15. **Escritor Intel HEX** con config words y checksum.
16. **Blink real en PIC16F887** (criterio MVP).

## Infraestructura (`documentation`, `good first issue`)
17. **CI en GitHub Actions:** `cargo test`, `cargo clippy`, `cargo fmt --check`.
18. **Licencia y CONTRIBUTING** (Fase 0, `TASKS.md`).
19. **Docs: sustituir "8 bits" / "PIC8" por "todos los PIC (PIC16/PIC18/PIC24/dsPIC/PIC32)"** en el `.docx`, README, AGENTS.md y los `0x_*.md`.
20. **Archivo `pypic.pyi`** con los nombres predeclarados (`u8`, `Const`, `interrupt`...) para editores y linters Python.
