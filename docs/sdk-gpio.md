# SDK: acceso a pines (`pypic.gpio`)

**Estado: primer corte implementado.** Todo se resuelve al compilar; el MCU ejecuta solo instrucciones nativas
(sin intérprete ni runtime Python). Los nombres siguen la idea de MicroPython (`on/off/toggle/value`) pero PyPIC **no** es MicroPython.

## Decisión de diseño: los pines son nombres predeclarados

```python
from pypic.device import PIC16F887
from pypic.gpio import RD0, RB0

def main() -> None:
    RD0.output()
    RB0.input()
    while True:
        if RB0.value() == 1:
            RD0.on()
        else:
            RD0.off()
```

Se descartó el estilo `led = Pin("RD0", Pin.OUT)` (el de MicroPython) para evitar confusiones:

- No hay cadenas ni constructores: un pin mal escrito es un **nombre no definido** y el compilador lo rechaza.
- `pypic.gpio` exporta **un nombre por pin de E/S del dispositivo** (según el Device Pack); importar uno que no existe es un error
  que lista los pines disponibles. Las patas de alimentación no se exportan.
- No hay objetos que se puedan crear en ejecución, pasar como argumento o guardar: cada `RD0` es el pin físico.

## API

| Llamada | Efecto |
|---|---|
| `RD0.output()` | Pin como salida (y lo pasa a digital si era analógico) |
| `RD0.input()` | Pin como entrada digital |
| `RD0.on()` / `RD0.off()` | Pone el latch en alto / bajo |
| `RD0.toggle()` | Invierte el latch (una sola instrucción de lectura-modificación-escritura) |
| `RD0.value()` | Lee el pin: `u8`, 0 o 1 |
| `RD0.value(x)` | Escribe el pin: `x` es `u8` o `bool`; distinto de 0 = alto. Sin parpadeos intermedios |

## Comprobaciones del compilador

| Código | Regla |
|---|---|
| `PYP2074` | El pin no existe en el dispositivo (el mensaje lista los disponibles) |
| `PYP2078` | Se usa un pin sin importarlo |
| `PYP2075` | `output()` sobre un pin solo de entrada (RE3 en el PIC16F887) |
| `PYP2079` | `on/off/toggle/value(x)` sin que el pin se configure como salida en ningún punto del programa |
| `PYP2081` | `value()` sin que el pin se configure (ni `input()` ni `output()`) |
| `PYP2077` | Un pin usado como valor (`x = RD0`) |
| `PYP2080` / `PYP2055` | Método inexistente / número de argumentos incorrecto |
| `PYP2009` | Un pin choca con otra variable o función del mismo nombre |

La comprobación de configuración es sobre **todo el programa**, sin importar el orden (se puede configurar en otra función).
No analiza el orden de ejecución: llamar `RD0.on()` antes de `RD0.output()` compila.

## Qué hace el compilador por ti (PIC16F887)

- **Arranque:** pone en 0 el latch de cada puerto usado, porque tras el reset es indefinido (DS40001291H p.27). Así las salidas empiezan en bajo.
- **`output()` / `input()`:** si el pin tiene función analógica (`ANn`) limpia su bit de `ANSEL`/`ANSELH` (banco 3, `188h`/`189h`); sin esto los pines
  analógicos **leen 0 aunque haya señal**. Luego fija `TRISx` (banco 1) y **vuelve siempre al banco 0** (invariante de la ABI).
- El dispositivo se declara al comienzo del programa (`from pypic.device import PIC16F887`, ver `docs/frontend.md`): de ahí sale el pack con los pines y sus canales analógicos.
- Lo natural es configurar los pines en una función `@init_device`, que el arranque ejecuta antes de `main`.

## Límites actuales

- Solo puertos A–E y bits 0–7 (PIC16F88x). Las direcciones de los registros están fijas en el backend; pasarán al Device Pack (`registers.json`, #22).
- No hay pull-ups débiles del puerto B, drenador abierto, interrupción por cambio ni funciones alternas (PWM, UART...).
- **Sin retardos todavía** (`sleep_ms`): un `toggle()` en bucle conmuta a MHz. El módulo de tiempo es el siguiente paso.
- Lectura-modificación-escritura: `on/off/toggle` usan `BSF/BCF/XORWF` sobre `PORTx`, que en PIC16 leen el *nivel del pin* de todo el puerto. Con cargas que
  distorsionen el nivel de otros pines de salida del mismo puerto puede cambiarlos.

## Cómo se verifica

Pruebas del frontend (diagnósticos), del Device Pack (pines reales del PIC16F887) y **diferenciales**: el mismo programa en el intérprete de la IR y, compilado, en
el simulador PIC14; se comparan **los cambios de nivel de cada pin en orden**, no solo el estado final (`crates/pypic-pic14/tests/gpio.rs`). El simulador
del host no sustituye a Proteus ni al hardware.

## Ejemplo para Proteus: `examples/button_led.py`

```
pypic build examples/button_led.py -o examples/hex/button_led.hex
```

Montaje: PIC16F887; LED + resistencia de 330 Ω entre **RD0 (pin 19)** y GND; un **pulsador en RB0 (pin 33)** hacia +5 V con una resistencia de **10 kΩ a GND**
(o un `LOGIC STATE`). Program File: `examples/hex/button_led.hex`. Al pulsar, el LED se enciende; al soltar, se apaga. No depende del reloj.
Estado: **no probado todavía en Proteus** (solo en el simulador del host).
