# Compilador, IR y generación de código

Pipeline: .py PyPIC -> parser/AST -> análisis de tipos -> semántica y validación del dispositivo -> IR tipada -> optimización -> backend ASM PIC14 / código máquina nativo o backend C -> ensamblador/linker correspondiente -> Intel HEX + mapa + símbolos. Tipos uint8, int8, uint16, int16, bool; considerar extensiones uint32/int32 y arreglos estáticos. Semántica explícita de overflow, conversiones, constantes, bitwise, loops, funciones, interrupciones y memoria estática. Definir ABI, registros temporales, W, STATUS, PCLATH, bancos, stack hardware, vectores, config words y placement de datos.

Backends: (1) PIC14 nativo con codificación de instrucciones y ensamblador propio, sin XC8; (2) ASM para PIC-AS; (3) C para XC8/SDCC mediante transpilación; (4) C++ solo cuando un toolchain y dispositivo lo soporten, no asumir C++ en XC8 PIC8. El IR común debe desacoplar sintaxis de destino. Toolchain adapter detect(), validate(), supported_devices(), build(), diagnostics(), debug_info(). Diferenciar fuente PyPIC, C generado, ASM generado y direcciones máquina; source maps para errores y debugging.

Inline ASM asm(...), @asm_function y módulos .asm: restricciones de arquitectura, clobbers, parámetros, retorno, efectos de flags, banco, interrupciones y compatibilidad del backend. Evitar prometer portabilidad de ASM arbitrario a C. HEX: validar direccionamiento y representación de palabras de programa de 14 bits, checksum y configuration words por dispositivo.

Optimizaciones: constant folding, eliminación de código muerto, propagación de constantes, uso eficiente de bancos, tamaño vs velocidad, advertencias de RAM/Flash, pila y timing. No atribuir optimizaciones no implementadas.

## Exclusión de intérprete/VM en el firmware
Todo parsing, análisis semántico, IR, transpilación y compilación ocurre en el **host**. El destino ejecuta instrucciones PIC nativas. Una posible biblioteca de soporte consiste únicamente en rutinas nativas estáticas; queda prohibido agregar bytecode Python, REPL o intérprete al dispositivo. Véase `00_PRINCIPIOS_NO_INTERPRETE.md`.
