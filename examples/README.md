# Ejemplos

Los ejemplos se organizan **por lenguaje** y, dentro de cada uno, **por funcionalidad** (los mismos nombres que los módulos del SDK en `docs/sdk-*.md`):

```
examples/
  python/                    Python (.py): el lenguaje principal
    gpio/                      pines, botones y contadores
    temporizadores/            Timer0, Timer1 y Timer2
    adc/                       convertidor analógico-digital
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
