# VS Code, Wizard y perfiles

Extensión VS Code: syntax highlighting, LSP, autocomplete, hovers de registros/datasheet, tareas build/check/flash, estado del MCU, explorador de memoria, pinout interactivo por encapsulado, conflictos visuales, perfiles Debug/Release, DAP y paneles de herramientas.

Project Wizard en cuatro modos: nuevo proyecto, abrir/validar, migrar MCU y reconfigurar. Pasos: nombre, dispositivo y encapsulado, oscilador/frecuencia/config bits, toolchain, pines/periféricos, programador/debugger, librerías, resumen/validación. Migraciones con análisis diferencial, propuestas de remapeo, previsualización de cambios, backup y confirmación explícita antes de modificar archivos. Nunca sobrescribir sin confirmación.

Configuración pyxpic.toml + lockfile; targets/*.toml para perfiles PIC16F887 y PIC12F625 sin alterar fuente principal. Distinguir opciones por proyecto y globales; detectar toolchains y mostrar rutas/versiones/compatibilidad. Generar reportes de pinout, memoria, build y configuración.

Toolchain Manager estilo Dev-C++: PyxPic Native predeterminado, PIC-AS para ASM, XC8/SDCC para C generado, terceros por adaptadores; verificar toolchain real y capacidades. Programadores y depuradores son providers separados del compilador.
