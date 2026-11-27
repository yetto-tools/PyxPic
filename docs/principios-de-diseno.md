# Principios de diseño: ensamblador de alto nivel

**PyxPic lleva el código de ensamblador a un lenguaje de alto nivel conservando la flexibilidad y la precisión de las instrucciones.**
La sintaxis es Python; el resultado son instrucciones nativas del PIC (nunca un intérprete ni un runtime en el MCU, ver `docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md`).
Quien hoy escribe en ensamblador debe poder escribir lo mismo aquí, con menos errores y más legible.

La experiencia inicial debe ser accesible como en Arduino, sin renunciar a las capacidades del PIC. Una operación completa y sus primitivas no son sinónimos si ofrecen contratos distintos: por ejemplo, una lectura que espera y las operaciones separadas para iniciar y consultar la conversión. La API debe indicar efectos, bloqueos, supuestos eléctricos y recursos, y ofrecer configuración explícita cuando corresponda.

El acceso a registros y a ensamblador en sintaxis PIC-AS permite un control progresivo. Como objetivo, el listado, el mapa de memoria y el informe de tiempos deben ayudar a inspeccionar el firmware generado. No se promete que una construcción de alto nivel produzca siempre una secuencia fija o ciclos exactos; eso debe especificarse y verificarse por operación. Ninguna de estas metas implica soporte completo ya implementado ni requiere GPASM o XC8.

## Filosofía

PyxPic busca extraer el máximo desempeño y aprovechamiento de los recursos físicos de los PIC, eliminando buena parte de la complejidad de programar directamente en ensamblador. Su sintaxis, de alto nivel, se inspira en Python y BASIC sin adoptar sus modelos de ejecución interpretada. **La complejidad debe residir en el compilador, no en el microcontrolador.**

Principios fundamentales:

- Maximizar el aprovechamiento de CPU, RAM, Flash y periféricos.
- Generar código nativo eficiente y predecible.
- Reducir la complejidad de programación sin reducir el control sobre el hardware.
- Trasladar la complejidad del firmware hacia el compilador.
- Priorizar el determinismo temporal y de memoria.
- Mantener la transparencia del código generado.
- Facilitar la depuración a nivel de código fuente, ensamblador, registros y memoria.
- Evitar intermediarios, capas de ejecución y abstracciones innecesarias.
- Resolver durante la compilación todo lo que no necesite resolverse durante la ejecución (las unidades de `docs/sdk-reloj.md` son un ejemplo: `11.0592MHz` es un entero exacto al compilar, sin punto flotante ni RAM).
- Respetar las capacidades y limitaciones físicas de cada microcontrolador: los límites salen del Device Pack del dispositivo, no de una tabla global de la familia.

### Criterio de incorporación de funcionalidades

Una característica solo se incorpora cuando da una ventaja práctica y demostrable sobre el hardware. No se incorporan funcionalidades que reduzcan injustificadamente los recursos disponibles, introduzcan latencias impredecibles, dificulten la depuración o comprometan la estabilidad del firmware. La calidad de una abstracción se evalúa con el ensamblador generado, los ciclos de ejecución, el consumo de memoria y su comportamiento sobre hardware real (hasta tener la prueba en hardware, el nivel de verificación se dice: simulador, Proteus o hardware).

### Bibliotecas

Las bibliotecas del SDK son nativas y optimizadas para la arquitectura PIC. Pueden tomar como referencia las interfaces de otros ecosistemas, como MicroPython, pero nunca sacrifican eficiencia ni determinismo para mantener la compatibilidad.

### Objetivo

La expresividad de un lenguaje de alto nivel, el control del ensamblador y la eficiencia de la compilación nativa.

## Reglas que se derivan

1. **Precisión.** El costo en ciclos es predecible, se puede conocer y se reporta. Una espera constante dura exactamente lo pedido (`docs/sdk-time.md`); `pyxpic build --timing` muestra lo emitido y el error.
2. **Flexibilidad.** Todo lo que el ensamblador alcanza debe ser alcanzable: registros, bits, banderas, memoria, periféricos, interrupciones. Lo que el lenguaje aún no cubre tiene una salida de emergencia (ensamblador en línea).
3. **Sin costos ocultos.** El código generado es legible (`--listing`), sin runtime, y las operaciones que necesitan una rutina nativa (`*`, `//`, `%`) la dicen: un aviso `PYP3011` con el nombre de la rutina, su costo documentado (`docs/sdk-runtime.md`) y la rutina visible en `--listing`.
4. **Una sola forma por operación.** Sin sinónimos, para evitar ambigüedades (`pause` y no `delay_ms`).
5. **Explícito antes que implícito.** Sin conversiones implícitas ni «verdad» de enteros; los tipos son de ancho fijo.
6. **Conocimiento del dispositivo.** El programa declara su PIC (`from pyxpic.device import ...`) y el compilador valida pines, memoria, reloj y periféricos contra el Device Pack, cosa que el ensamblador no hace.

## Inventario: qué hace el ensamblador y qué cubre PyxPic hoy

| Capacidad del ensamblador | Estado en PyxPic |
|---|---|
| Aritmética y lógica de 8/16 bits, comparaciones, saltos, llamadas, bucles | ✅ implementado (verificado contra el simulador) |
| Control de pines (TRIS, PORT, ANSEL) | ✅ `pyxpic.periph.gpio` |
| Esperas exactas al ciclo | ✅ `pyxpic.system.time` (`pause`, `wait`, `delay_us`, `delay_cycles`) |
| Interrupciones con guardado de contexto | ✅ `@interrupt` |
| Arranque y configuración inicial | ✅ `@init_device`, `CLOCK` (fuente y frecuencia del reloj), declaración del dispositivo |
| Bajo consumo (`SLEEP`, watchdog) | ⏳ `nap`/`sleep` reservadas (#38) |
| Timers (prescaler, desbordamiento, interrupciones) | ✅ `pyxpic.periph.timer`, `docs/sdk-timer.md` (#39) |
| **Acceso directo a cualquier registro (SFR) y a sus bits** | ✅ `pyxpic.regs`, `docs/sdk-regs.md` (#40) |
| **Rotación por el acarreo, `SWAPF`, prueba/puesta de bits y banderas C/Z/DC** | ✅ `pyxpic.bits`, `docs/sdk-bits.md` (#41); Z y DC con contrato: ⏳ #59 |
| Ensamblador textual de las 35 instrucciones (`pyxpic asm`), con la sintaxis oficial de **pic-as** (XC8) | ✅ `docs/asm-pic14.md` |
| **Ensamblador en línea (`@asm_function`)**: la salida de emergencia | ✅ `docs/asm-inline.md` (pendiente en #42: llamadas a otras funciones y desde la interrupción) |
| **Análisis estático de ciclos por función y contratos de tiempo máximo** (p. ej. «esta interrupción dura ≤ N ciclos») | ⏳ #43 |
| Tablas constantes en flash (`RETLW`, `GOTO` calculado) | ✅ `Rom[uint8]`, `docs/sdk-tablas.md` (#44) |
| Arreglos y direccionamiento indirecto (`FSR`/`INDF`) | ⏳ #8 |
| Multiplicar/dividir/módulo y desplazar por una variable (rutinas nativas) | ✅ `docs/sdk-runtime.md` (#27) |
| Varios bancos de datos y solapamiento de locales | ⏳ #26 |
| Páginas de programa (`PCLATH`) para más de 2048 palabras | ⏳ #31 |
| EEPROM de datos, ADC, UART, I²C, PWM... | ⏳ periféricos (#23) |

La columna de estado es la hoja de ruta: cada fila pendiente tiene su issue.
