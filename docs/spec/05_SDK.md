# SDK, librerías y programación visual

SDK propio: machine, gpio, time, adc, pwm, timer, uart, spi, i2c, eeprom, interrupts, math, bits; drivers de displays (7 segmentos, LCD 16x2, matrices), memorias y sensores. API homogénea con variantes hardware/software y errores de compatibilidad. Intrinsics del compilador para operaciones especiales. Paquetes con manifiesto, dependencias, versionado, compatibilidad de MCU y registro comunitario futuro.

Programación visual v2.1: Blockly/Scratch/PICAXE, NO ladder ni FBD PLC Siemens. Bloques -> AST/IR -> firmware; sincronización bloques-PyxPic limitada al subconjunto reversible; depuración de bloques y fuente; bloques conscientes del hardware. Ejemplos y plantillas educativos.

Ejemplo API conceptual: from machine import Pin, I2C, UART; led=Pin("RB0",Pin.OUT); i2c=I2C(1,freq=100000); uart=UART(1,baudrate=9600). Las firmas son propuestas sujetas a diseño, no API ya implementada.
