' Cuenta los arranques en la EEPROM (direccion 0) y parpadea RD0 tantas veces como arranques lleva: el contador sobrevive al apagado.
' Equivale a examples/python/eeprom/eeprom_arranques.py.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ
FROM pypic.periph.eeprom IMPORT EEPROM

SYMBOL led = D.0

b0 = EEPROM.read(0)         ' una EEPROM virgen vale 255
IF b0 = 255 THEN b0 = 0
b0 = b0 + 1
EEPROM.write(0, b0)         ' espera hasta 6 ms; el byte se borra y se escribe solo
b1 = 0
DO WHILE b1 < b0
  HIGH led
  PAUSE 200
  LOW led
  PAUSE 200
  b1 = b1 + 1
LOOP
END
