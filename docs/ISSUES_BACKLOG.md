# Historia del backlog

Este archivo era un borrador de issues. Los issues ya existen en GitHub (`yetto-tools/PyPic`) y **la fuente de verdad es GitHub**; aquí queda el mapa de lo que se propuso al empezar y en qué quedó,
verificado el 2026-11-24. Para el estado por componente, ver [`TASKS.md`](../TASKS.md).

## Decisiones de diseño — resueltas
| # | Decisión | Dónde quedó |
|---|---|---|
| #1 | Tipos anchos en MCU pequeños: se permiten y se emulan, con aviso de costo (`PYP3001`) | `docs/frontend.md` |
| #2 | Semántica fijada (`//` y `%` truncan hacia cero, condiciones solo `bool`, conversiones explícitas, overflow constante = error) | `docs/frontend.md`, `docs/sdk-runtime.md` |
| #3 | Reentrancia con memoria estática: una función no se comparte entre `main` y la interrupción (`PYP2060`) | `docs/abi-pic14.md` |

## Hecho
| # | Qué | Dónde |
|---|---|---|
| #7 | Intérprete de IR de referencia (solo host, oráculo de las pruebas diferenciales) | `crates/pypic-ir` |
| #9 | Resolución de módulos del SDK contra el Device Pack | `docs/sdk-modulos.md` |
| #12 | Esquema y validación del Device Pack; `TargetProfile` desde el pack | `docs/device-packs.md` |
| #13 | ABI PIC14: temporales, W/STATUS/PCLATH, bancos, vectores | `docs/abi-pic14.md` |
| #14 | Codificador de instrucciones y ensamblador propio (sintaxis de pic-as) | `docs/asm-pic14.md` |
| #15 | Escritor/lector Intel HEX con palabras de configuración y checksum | `crates/pypic-pic14/src/hex.rs` |
| #22, #23 | Device Pack del PIC16F887: registros y periféricos | `devices/pic16f887` |

Además se hicieron, sin estar en este borrador: temporizadores (#39), registros (#40), bits y acarreo (#41), configuración por nombre (#33), conflictos de pines (#54), tablas en flash (#44),
rutinas de runtime (#27), macros del ensamblador (#58) y el arreglo del lector HEX (#61).

## Abierto (los números son los de GitHub)
- **Compilador / IR:** #4 errores de tipos junto a errores de sintaxis · #5 advertencias de código inalcanzable y variables sin usar · #6 optimizaciones de IR · #8 arreglos estáticos · #10 diagnósticos en JSON · #11 constantes locales sin RAM · #43 análisis de ciclos
- **Backend:** #26 overlay de memoria · #31 `PCLATH` (>2048 palabras) · #32 `uint32`/`int32` · #25 PIC12F625 · #24 encapsulados de 44 pines
- **SDK:** #45 periféricos (ADC, PWM, UART, SPI, I²C...) · #46 `wdt`, `sleep`, reloj · #38 `nap`/`sleep` · #47 protocolos por software · #36 pull-ups e interrupción por cambio · #53 puertos completos · #59 banderas Z y DC
- **Ensamblador:** #48 validar contra el Device Pack · #49–#52 y #60 restos del ensamblador en línea y de pic-as · #55 exportar en pic-as · #56 migración desde MPASM · #57 guía de Python a ensamblador
- **Infraestructura y documentación:** #16 blink en hardware real · #17 CI · #18 licencia y CONTRIBUTING · #19 quitar «8 bits» de la documentación · #20 y #35 stubs `.pyi` · #37 `pypic.toml` · #62 (este archivo, el README y `TASKS.md`)
