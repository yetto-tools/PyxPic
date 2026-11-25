# PyPIC IDE para VS Code

**Versión de la extensión: 0.1.14.** VS Code muestra este número en el resumen y desde el manifiesto de la extensión en la vista Extensiones.

Extensión para VS Code de escritorio. Usa el compilador PyPIC instalado en el host; no necesita GPASM ni XC8 para crear, comprobar y compilar proyectos.

1. Compila la CLI con `cargo build -p pypic-cli` desde la raíz del repositorio.
2. Abre `vscode-pypic` en VS Code y pulsa F5 para iniciar una ventana de desarrollo de extensiones. Abre allí el repositorio o tu proyecto PyPIC en un espacio de trabajo de confianza.
3. Al abrir un ejemplo dentro del repositorio, la extensión busca la CLI compilada y sus Device Packs automáticamente. También puedes usar **Seleccionar compilador** y **Seleccionar Device Packs** en la barra lateral. Se guardan por carpeta del proyecto cuando el archivo activo pertenece a una carpeta abierta; si no, en los ajustes del usuario. Cancelar no modifica la configuración.
4. Selecciona el icono PyPIC de la barra de actividad y abre **Inicio de PyPIC**. Desde allí puedes crear un proyecto, abrir un ejemplo y ver si el compilador y los Device Packs están listos.
   La sección **Pinout** permanece visible en la misma barra lateral y sigue el PIC declarado en el archivo o el perfil de proyecto activo.
5. **Crear proyecto** pide el nombre, carpeta, PIC16F887 y reloj. Ejecuta `pypic init`: crea `pypic.toml` (dispositivo, reloj y salida) y un `main.py` válido, sin sobrescribir carpetas existentes; al finalizar puedes abrir la nueva carpeta como proyecto de VS Code.
6. Abre un archivo `.py` y usa **Verificar código** o **Compilar a HEX**. Ambos botones aparecen juntos en el editor; los mensajes se muestran en Salida → PyPIC y los diagnósticos en Problemas.
7. **PyPIC: Abrir ejemplo** busca archivos Python bajo carpetas `examples` del espacio de trabajo abierto.

## Archivo suelto o proyecto

La extensión distingue dos formas de trabajar; la barra de estado dice cuál está activa (`PyPIC · archivo suelto` o `PyPIC · proyecto nombre · perfil`) y al pulsarla se puede cambiar.

| | **Archivo suelto** (**Abrir archivo…**) | **Proyecto** (**Abrir proyecto…**) |
|---|---|---|
| Qué es | Un `.py` por sí solo, como siempre. | Una carpeta con un `pypic.toml` (se busca desde el archivo hacia arriba). |
| Compilar | **Pregunta dónde guardar** el `.hex`. | **No pregunta nada**: usa el perfil de destino (dispositivo, reloj, configuración) y su salida, por ejemplo `build/nombre.hex`. |
| Comprobar | Con el dispositivo que declare el archivo. | Con el dispositivo y el reloj del perfil. |
| Perfil | No hay. | **Perfil de destino** elige uno (se recuerda por proyecto); **Revisar proyecto** ejecuta `pypic project check`. |

**Abrir archivo…** abre un `.py` como archivo suelto aunque esté dentro de un proyecto (en la barra de estado también hay **Tratar este archivo como archivo suelto** y **Usar el proyecto de este archivo**). **Abrir proyecto…** pide la carpeta con `pypic.toml`: si no la tiene, ofrece crear el proyecto ahí.

**Al compilar o comprobar, el panel Salida se abre y se selecciona solo** (canal PyPIC), sin quitar el foco del editor, y muestra el proyecto, el perfil, el dispositivo, el reloj y la ruta del `.hex`; el aviso final dice dónde quedó el archivo.

La sección persistente **Recursos** compara el uso medido durante la última compilación con la ROM y RAM disponibles del Device Pack. También muestra RAM total, EEPROM y los periféricos referenciados por los imports del archivo frente al catálogo del PIC.

Los archivos Python con imports `from pypic... import ...` se abren en el modo PyPIC: se mantiene el resaltado de Python y se evita aplicarles los diagnósticos de Pylance para Python convencional. **Comprobar archivo** y **Compilar** muestran en Problemas los errores y avisos del compilador; al editar se eliminan y al guardar se comprueba el archivo de nuevo. La opción `pypic.checkOnSave` desactiva esa comprobación. Si la CLI no está configurada, no hay diagnóstico semántico automático: consulta Salida → PyPIC. El texto emergente ofrece una ayuda inicial para tipos y funciones comunes, y el autocompletado propone tipos y métodos de ADC y UART; todavía no cubren toda la API. Para volver manualmente a Python, usa el selector de lenguaje de VS Code.

El autocompletado y la ayuda también incluyen `CLOCK`, `Internal`, `Crystal`, `ExternalClock`, `Hz`, `kHz` y `MHz`. La configuración real del reloj se describe en [la guía del SDK](../docs/sdk-reloj.md).

El pinout se elige automáticamente desde el código fuente y, si no hay declaración, desde el perfil activo de `pypic.toml`. El selector permanece disponible para inspeccionar cualquier Device Pack instalado y permite volver a **Automático**; esa elección solo cambia la vista y no modifica el proyecto. Esta versión no implementa comprobación continua mientras se escribe, informe de memoria, grabación, terminal serie, simulación visual ni depuración. El comando de compilación puede sustituir el HEX seleccionado; el diálogo de guardado es el del editor.

**Seleccionar PIC-AS (opcional)** guarda su ruta para referencia; la extensión todavía no ejecuta PIC-AS ni integra otros toolchains. PyPIC no necesita XC8 para compilar.

Para verificar las pruebas: `npm test` dentro de `vscode-pypic` (o `node --test vscode-pypic/test/commands.test.js vscode-pypic/test/projectmode.test.js` desde la raíz; pasar la carpeta entera falla en Node 24). Necesitan la CLI compilada (`cargo build -p pypic-cli`).
