# Decisión arquitectónica innegociable: sintaxis Python, NO intérprete

**PyxPic utiliza únicamente una sintaxis inspirada en Python como lenguaje fuente. Llevar, portar, integrar o ejecutar un intérprete de Python en el microcontrolador NO es objetivo del proyecto, ni actual ni futuro.**

## Lo que PyxPic SÍ hace
- Analiza **en la computadora de desarrollo** un subconjunto estático y documentado de sintaxis Python (`.py`).
- Comprueba tipos, periféricos, memoria y restricciones del PIC antes de compilar.
- Transforma el código a una representación intermedia (IR), la optimiza y genera **código máquina nativo** del PIC (directamente o vía ensamblador).
- Puede transpilar el mismo programa a C, ASM o, cuando haya toolchain compatible, C++ para compilarlo **en el equipo anfitrión** con herramientas opcionales.
- El firmware HEX resultante se ejecuta directamente en la CPU del PIC, sin intérprete ni VM de Python.

## Lo que PyxPic NO hace y NO debe introducirse
- No portar CPython, MicroPython, CircuitPython ni otra implementación de intérprete al PIC.
- No evaluar scripts Python durante la ejecución en el MCU, no bytecode Python, no máquina virtual de Python.
- No cargar módulos `.py` dinámicamente en el microcontrolador ni incorporar REPL Python en el firmware.
- No requerir recolector de basura, sistema de objetos dinámico, reflexión ni biblioteca estándar de CPython en el MCU.
- No considerar «hacer correr Python en el PIC» un hito futuro; el lenguaje fuente está inspirado en su **sintaxis**, no en reproducir su runtime.

## Terminología obligatoria
- Correcto: «lenguaje de sintaxis Python compilado AOT», «subconjunto Python → firmware nativo», «transpilación en el host».
- Incorrecto: «Python interpretado en PIC», «MicroPython para PIC», «intérprete PyxPic dentro del MCU».
- El runtime de soporte **opcional y mínimo** (por ejemplo, rutinas de división o acceso a periféricos) es código máquina nativo enlazado, **nunca** un runtime/intérprete de Python.

## Criterio de aceptación
Un programa PyxPic de ejemplo debe producir un HEX autónomo que ejecute en el PIC especificado sin instalar ni incorporar intérprete/VM Python. Las pruebas de arquitectura deben rechazar dependencias de intérprete en el firmware.
