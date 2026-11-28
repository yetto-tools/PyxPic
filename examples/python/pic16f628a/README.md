# Blink en el PIC16F628A

```
pyxpic build examples/python/pic16f628a/blink.py -o examples/python/pic16f628a/blink.hex
```

Conexión (PDIP-18): pin 14 (VDD) a 5 V, pin 5 (VSS) a tierra, pin 6 (RB0) -> 330 ohm -> LED -> tierra. Un condensador de 100 nF entre VDD y
VSS junto al chip. Reloj interno de 4 MHz: no hace falta cristal.

Al grabar `blink.hex` elige **PIC16F628A** en el programador y comprueba que la configuración quede en `0x3F18`.
Probado en el simulador y en hardware (2026-11-28: el LED parpadea). Limitaciones del chip en `docs/device-packs.md`.
