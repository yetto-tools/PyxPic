# Device Packs y compatibilidad

Device Packs versionados con device.json, memory.json, registers.json, pins.json, peripherals.json, config.json y metadata de ISA. Reutilizar definición de CPU, MCU y modelos de periféricos. Importar metadatos DFP oficiales cuando licencia y formato lo permitan; validar contra datasheets. Comandos pyxpic device create/import/validate/test/list. Niveles de soporte: experimental, compila, simula, probado en hardware, depuración física.

PIC16F887: arquitectura mid-range PIC14 clásica; 8K palabras de programa de 14 bits; 368 B RAM GPR; 256 B EEPROM; bancos y MSSP, EUSART, ADC, CCP. PIC12F625: PIC14 clásica; 1K palabras; 64 B RAM; 128 B EEPROM; sin ADC ni UART hardware. Revisar datasheets para detalles exactos de pines, osciladores y config bits. Familias futuras: baseline 12-bit, enhanced baseline, mid-range 14-bit, enhanced mid-range, PIC18 16-bit.

Analizador LSP: instrucciones inválidas para CPU (p.ej. MOVLB en PIC16F887 clásico), periféricos ausentes, pines imposibles, conflictos GPIO/CCP, relojes incompatibles, overflow, límites de RAM/Flash, pila, ISR, compatibilidad toolchain/MCU. Soporte de comparación entre targets y propuestas de migración.
