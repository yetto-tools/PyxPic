# Árbol de módulos del SDK

**Estado: registro de módulos implementado; los módulos marcados «reservado» se reconocen pero todavía no existen.**
Todo vive bajo `pypic.*` (no `machine`, que es el módulo de MicroPython: PyPIC no es MicroPython). Los módulos se agrupan en **tres categorías**
para que al leer un `import` se sepa qué se está usando: hardware del chip, servicio del sistema o implementación por software.

```python
from pypic.device import PIC16F887            # declaración del dispositivo (siempre la primera línea)
from pypic.periph.gpio import RD0, RB0        # periférico: hardware del chip
from pypic.system.time import pause           # sistema: CPU, reloj y energía
# from pypic.soft.spi import ...              # software: sobre cualquier GPIO, cuesta ciclos de CPU
```

| Categoría | Qué es | Módulos |
|---|---|---|
| `pypic.periph.*` | **Hardware integrado del chip**; solo existe si el dispositivo lo tiene (lo valida el Device Pack) | `gpio` ✅, `timer` ✅ (`docs/sdk-timer.md`), `adc` ✅ (`docs/sdk-adc.md`), `uart` ✅ (`docs/sdk-uart.md`), `pwm`, `ccp`, `eccp`, `spi`, `i2c`, `comparator`, `vref`, `eeprom`, `latch` |
| `pypic.regs` | **Registros del núcleo y de los periféricos por nombre** (`STATUS.Z.set()`, `OPTION_REG.write(v)`), validados con `registers.json` | ✅ `docs/sdk-regs.md` |
| `pypic.bits` | **Rotaciones, SWAPF, bits y acarreo**: intrínsecos 1:1 a instrucción (`rol_c`, `swap`, `bit_set`...) | ✅ `docs/sdk-bits.md` |
| `pypic.system.*` | **CPU, reloj y energía** | `time` ✅ (`pause`, `wait`, `delay_us`, `delay_cycles`; `nap`/`sleep` reservadas), `irq`, `wdt`, `clock`, `power`, `flash` |
| `pypic.soft.*` | **Implementaciones por software** de protocolos, sobre cualquier GPIO | `spi`, `i2c`, `uart`, `onewire` |

✅ = implementado. El resto son rutas reservadas: importarlas da `PYP2105` («todavía no está implementado»).

## Reglas

- **Hardware y software son módulos distintos.** `pypic.periph.spi` usa el MSSP del chip; `pypic.soft.spi` genera el protocolo con instrucciones sobre los pines que se elijan.
  El compilador **nunca elige por ti**: si pides el hardware y el chip no lo tiene, da error y sugiere la versión de `pypic.soft`. Así el costo en ciclos siempre se ve en el `import`.
- **Una sola ruta por cosa.** `pypic.gpio` y `pypic.time` ya no existen (`PYP2106` indica la ruta nueva).
- **Cada categoría solo tiene sus módulos.** No hay GPIO «por software» ni 1-Wire por hardware; pedirlos da `PYP1002` con la lista de lo que sí hay en esa categoría.
- Una categoría no se importa como módulo (`from pypic.periph import gpio` da `PYP1002`).
- Los protocolos que el chip no soporta (CAN, USB, I²S) no tienen módulo. Los que requieren un transceptor externo (RS-232, RS-485) usan `pypic.periph.uart` y lo advertirán.

## Diagnósticos

| Código | Regla |
|---|---|
| `PYP2105` | Módulo reconocido pero todavía no implementado |
| `PYP2106` | Ruta antigua: se llama ahora de otra forma |
| `PYP1002` | Módulo inexistente, o una categoría usada como módulo |

## Pendiente

Cada módulo reservado se implementa con su issue: `nap`/`sleep` (#38), y el resto del catálogo (`docs/catalogo-pic16f887.md`, #23). Todos dependen de que el Device Pack
describa los periféricos y los registros (`peripherals.json`, `registers.json`, #22/#23): ahí se declara qué módulos de `pypic.periph` existen en cada dispositivo.
