# Ejemplos

Los ejemplos se organizan **por lenguaje** y, dentro de cada uno, **por funcionalidad** (los mismos nombres que los módulos del SDK en `docs/sdk-*.md`):

```
examples/
  python/                    Python (.py): el lenguaje principal
    gpio/                      pines, botones y contadores
    temporizadores/            Timer0, Timer1 y Timer2
    adc/                       convertidor analógico-digital
    i2c/                       bus I2C con el MSSP (LCD 16x2 con mochila PCF8574; docs/sdk-i2c.md)
    uart/                      puerto serie (ver su README)
    eeprom/                    EEPROM de datos
    pwm/                       PWM de CCP1 y CCP2
    comparadores/              comparadores y CVREF
    registros/                 registros, bits y acarreo
    tablas/                    tablas en memoria de programa
    reloj/                     fuentes de reloj (cristal, interno)
    asm-en-linea/              @asm_function
    hardware/                  programas de prueba en hardware real (K150, docs/programador-k150.md)
    proyecto/                  un proyecto con pypic.toml y dos perfiles
  basic/                     PyPIC BASIC (.bas, estilo PICAXE; docs/basic-picaxe.md)
    gpio/
    adc/
  asm/                       ensamblador PIC-AS (.s, docs/asm-pic14.md)
    gpio/
  hex/                       HEX ya compilados (los verificados en Proteus: docs/proteus-blink.md)
```

Se compilan con `pypic build <ruta> -o salida.hex` (la extensión del archivo elige el lenguaje; `.s` va con `pypic asm`). Las pruebas del repositorio compilan y simulan estos ejemplos: al agregar uno, ponlo en la carpeta de su lenguaje y su funcionalidad, y si es un programa de Python, que declare su dispositivo (`from pypic.device import PIC16F887`).

## Los mismos programas en los tres lenguajes

Varios ejemplos existen en los tres lenguajes y hacen lo mismo (cada uno nombra a su contraparte en el comentario del principio). Las pruebas los simulan y comprueban que se comportan igual:

| Programa | Python | BASIC | Ensamblador |
|---|---|---|---|
| Pulsador enciende un LED | `python/gpio/button_led.py` | `basic/gpio/boton_led.bas` | `asm/gpio/boton_led.s` |
| Parpadeo | `python/gpio/blink.py` | `basic/gpio/parpadeo.bas` | `asm/gpio/blink_rd0.s` |
| ADC: umbral en AN0 | `python/adc/adc.py` | `basic/adc/adc.bas` | `asm/adc/adc.s` |
| Contador de arranques en la EEPROM | `python/eeprom/eeprom_arranques.py` | `basic/eeprom/eeprom_arranques.bas` | `asm/eeprom/eeprom_arranques.s` |
| Eco por el puerto serie | `python/uart/eco.py` | `basic/uart/eco.bas` | `asm/uart/eco.s` |
| PWM con un potenciometro | `python/pwm/pwm.py` | `basic/pwm/pwm.bas` | `asm/pwm/pwm.s` |
| Pantalla LCD 16x2 por I2C | `python/i2c/lcd_i2c.py` | — | — |
| LCD 16x2 con controlador I2C propio (0x3E) | `python/i2c/lcd_controlador.py` | — | — |
| Busqueda de direcciones I2C (informa por la UART) | `python/i2c/lcd_buscar.py` | — | — |
| Saludo con un contador por la UART | `python/uart/saludo_texto.py` | `basic/uart/saludo.bas` | — |
| Media móvil del ADC | `python/adc/media_movil.py` | `basic/adc/media_movil.bas` | — |
| Comparador contra CVREF | `python/comparadores/comparador.py` | `basic/comparadores/comparador.bas` | — |
| Parpadeo con Timer2 | `python/temporizadores/timer_blink.py` | `basic/temporizadores/timer_blink.bas` | — |
| Cristal de 20 MHz | `python/reloj/reloj_cristal.py` | `basic/reloj/reloj_cristal.bas` | — |

Sin contraparte, porque el lenguaje todavía no lo soporta: tablas en flash, registros y bits, ensamblador en línea, contador con interrupción y los ejemplos de texto por la UART (BASIC: ver `docs/basic-picaxe.md`); y los de `python/hardware` y `python/proyecto`, que son pruebas con un programador real y un proyecto de dos perfiles.

