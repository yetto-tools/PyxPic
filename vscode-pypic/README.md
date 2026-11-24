# PyPIC IDE para VS Code

Extensión inicial para VS Code de escritorio. Usa el compilador PyPIC instalado en el host; no necesita GPASM ni XC8.

1. Compila la CLI con `cargo build -p pypic-cli` desde la raíz del repositorio.
2. Abre `vscode-pypic` en VS Code y pulsa F5 para iniciar una ventana de desarrollo de extensiones. Abre allí el repositorio o tu proyecto PyPIC en un espacio de trabajo de confianza.
3. Configura `pypic.executable` con la ruta absoluta a `target/debug/pypic` (en Windows, `pypic.exe`). Para proyectos fuera del repositorio, configura `pypic.devicesDir` con la ruta absoluta a `devices`.
4. Abre un archivo `.py` y usa los botones del editor o la paleta: **PyPIC: Comprobar archivo** y **PyPIC: Compilar archivo a HEX**. La compilación solicita el destino del HEX. Los mensajes aparecen en Salida → PyPIC.
5. **PyPIC: Abrir ejemplo** busca archivos Python bajo carpetas `examples` del espacio de trabajo abierto.

El dispositivo se declara en el código fuente, según las reglas actuales de PyPIC. VS Code proporciona el resaltado Python; esta versión no implementa autocompletado específico, diagnósticos en línea, informe de memoria, selección gráfica de dispositivo, grabación, terminal serie, simulación visual ni depuración. Esas funciones siguen pendientes. El comando de compilación puede sustituir el HEX seleccionado; el diálogo de guardado es el del editor.

Para verificar las pruebas: `node --test vscode-pypic/test/*.test.js` desde la raíz.
