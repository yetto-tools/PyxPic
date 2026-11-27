# Simulador y depuración

Simulador propio de CPU PIC14 con memoria, bancos, W, STATUS, PCLATH, pila, interrupciones, timers, GPIO y periféricos incrementales. Ejecutar paso a paso, breakpoints Python y ASM, memoria RAM/Flash/EEPROM, registros, watches, trazas y ciclos de CPU. Archivo .pyxpicdbg con mapeo línea->instrucciones y símbolos; adaptador DAP VS Code. Hardware debug opcional y condicionado a capacidades reales del PIC y del programador.

Analizador lógico digital con GPIO/PWM/UART/SPI/I2C y exportación VCD; decodificadores; componentes virtuales (LED, pulsador, displays, LCD, sensores simulados). No confundir con simulación analógica SPICE. Cobertura, pruebas deterministas, snapshots y depuración inversa a futuro. Serial plotter, medición de tiempo/ciclos, profundidad de pila, reportes de uso de memoria y estimación energética aproximada.
