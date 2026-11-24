# Visión y alcance

PyPIC es un proyecto open source de lenguaje Python restringido, compilado AOT a código máquina nativo para microcontroladores PIC de cualquier familia (PIC16, PIC18, PIC24, dsPIC y PIC32), sin intérprete ni runtime MicroPython en el MCU. Incluye CLI, SDK, extensiones de VS Code y herramientas de hardware. Objetivo inicial PIC16F887; segundo objetivo PIC12F625 (ambos de 8 bits); ampliar a las demás familias PIC mediante arquitectura extensible. Diferenciar entre requisitos aprobados, propuestas y funciones implementadas (ninguna implementación se presupone).

Principios: independencia de XC8; toolchains externos opcionales; código reproducible; diagnósticos conscientes del hardware; diseño modular; transparencia en compatibilidad y licencias.

Usuarios: estudiantes, aficionados, profesionales de firmware y colaboradores comunitarios. No pretende compatibilidad con todo Python: sin objetos dinámicos arbitrarios, reflexión, GC, imports generales de CPython ni bibliotecas Python de escritorio.

## Regla de alcance no negociable
**Solo adoptamos la sintaxis de Python; jamás se pretende portar un intérprete Python al PIC.** Ver `00_PRINCIPIOS_NO_INTERPRETE.md`.
