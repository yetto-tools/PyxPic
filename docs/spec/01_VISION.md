# Visión y alcance

PyxPic es un proyecto open source de lenguaje Python restringido, compilado AOT a código máquina nativo para microcontroladores de toda la familia PIC (de 8, 16 y 32 bits: PIC16, PIC18, PIC24, dsPIC y PIC32), sin intérprete ni runtime MicroPython en el MCU. Incluye CLI, SDK, extensiones de VS Code y herramientas de hardware. Objetivo inicial PIC16F887; segundo objetivo PIC12F625; ampliar a otras familias PIC8 mediante arquitectura extensible. Diferenciar entre requisitos aprobados, propuestas y funciones implementadas (ninguna implementación se presupone).

Principios: independencia de XC8; toolchains externos opcionales; código reproducible; diagnósticos conscientes del hardware; diseño modular; transparencia en compatibilidad y licencias.

Usuarios: estudiantes, aficionados, profesionales de firmware y colaboradores comunitarios. No pretende compatibilidad con todo Python: sin objetos dinámicos arbitrarios, reflexión, GC, imports generales de CPython ni bibliotecas Python de escritorio.

## Facilidad de uso y control del hardware

PyxPic busca la facilidad de inicio y de desarrollo de Arduino, conservando como objetivo el acceso a toda la capacidad del PIC y el control que ofrece su ensamblador. El usuario debe poder comenzar con ejemplos cortos y avanzar gradualmente hacia un control explícito del hardware dentro de un mismo proyecto:

1. Operaciones de alto nivel para tareas habituales, con valores predeterminados documentados y configurables.
2. Primitivas de periféricos para controlar cada paso, incluidos estados, esperas y operaciones no bloqueantes cuando existan.
3. Acceso a registros, bits, banderas, memoria e interrupciones, sujeto a las características del dispositivo.
4. Ensamblador en línea y módulos de ensamblador con sintaxis PIC-AS, integrados mediante un contrato de llamadas documentado.

Estos niveles son objetivos de diseño; su disponibilidad depende de la implementación y del Device Pack. El compilador debe mostrar los recursos utilizados, diagnosticar conflictos y límites, y permitir inspeccionar el ensamblador generado, el mapa de memoria y los costos temporales cuando estén disponibles. Las operaciones de alto nivel no prometen una secuencia de instrucciones ni un costo exactos salvo que su contrato lo indique; el ASM explícito permite controlar esas secuencias conforme al contrato del backend.

El primer objetivo de experiencia es crear, compilar, simular y, cuando exista soporte de programación, ejecutar ejemplos de GPIO, ADC y UART en un PIC16F887. La validación en simulador no sustituye las pruebas en hardware. El ensamblador propio usa sintaxis PIC-AS sin requerir GPASM ni una instalación obligatoria de XC8.

## Regla de alcance no negociable
**Solo adoptamos la sintaxis de Python; jamás se pretende portar un intérprete Python al PIC.** Ver `00_PRINCIPIOS_NO_INTERPRETE.md`.
