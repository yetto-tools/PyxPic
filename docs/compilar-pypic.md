# Compilar PyPIC desde el código fuente

Esta guía construye el compilador y ejecuta sus pruebas en el equipo de desarrollo. El resultado de compilar un programa PyPIC es firmware nativo para el PIC, sin intérprete Python en el microcontrolador.

## Requisitos

- [Rust y Cargo](https://www.rust-lang.org/tools/install), con un compilador nativo configurado para tu sistema. En Windows puede ser necesario instalar las herramientas de compilación de C++ que indique Rust.
- El código fuente de este repositorio y acceso a las dependencias de Cargo en la primera compilación.
- Para trabajar en la extensión: VS Code de escritorio; Node.js para ejecutar sus pruebas.

PyPIC no requiere GPASM, MPLAB X ni XC8 para construirse. PIC-AS y el Device Pack oficial son opcionales para las comparaciones con Microchip.

## 1. Abrir el proyecto

Abre una terminal en la carpeta raíz de PyPIC, la que contiene `Cargo.toml`, `crates/` y `devices/`. Comprueba las herramientas:

```text
rustc --version
cargo --version
```

## 2. Compilar todos los paquetes

```text
cargo build --workspace
```

Cargo compila el frontend, la IR, los Device Packs, el backend PIC14 y la CLI. La compilación de desarrollo produce:

| Sistema | Ejecutable |
|---|---|
| Windows | `target/debug/pypic.exe` |
| Linux o macOS | `target/debug/pypic` |

Para generar una versión optimizada:

```text
cargo build --workspace --release
```

El ejecutable queda en `target/release/`, con `.exe` en Windows. Compilar solo la CLI y sus dependencias también es posible con `cargo build -p pypic-cli`.

## 3. Ejecutar las pruebas

Desde la raíz:

```text
cargo test --workspace
```

Las pruebas del workspace incluyen pruebas del frontend, IR, Device Pack, backend y CLI. Algunas comparaciones con PIC-AS se **omiten** si no se configuran las rutas de XC8 y del Device Pack oficial. Para habilitarlas, consulta [Comparación con XC8 PIC-AS](asm-pic14.md#comparación-con-xc8-pic-as).

## 4. Compilar un programa de ejemplo

Desde la raíz, comprueba el programa y genera un HEX:

```text
cargo run -p pypic-cli -- check examples/uart/adc_serial.py
cargo run -p pypic-cli -- build examples/uart/adc_serial.py -o adc_serial.hex
```

`cargo run` construye la CLI si hace falta y luego la ejecuta. El HEX se crea en la ruta indicada; si el archivo ya existe, puede sobrescribirse. La salida muestra los recursos conocidos y las advertencias. El PIC16F887 se declara en el propio ejemplo, y la CLI encuentra su Device Pack en `devices/` cuando se ejecuta desde la raíz.

Para usar el ejecutable desde otra carpeta, configura `PYPIC_DEVICES` con la ruta absoluta a `devices/`, o pasa `--devices-dir <ruta>` al comando. La compilación y las pruebas no equivalen a grabar ni verificar el firmware en hardware.

## 5. Extensión de VS Code

La extensión se desarrolla por separado; `cargo build --workspace` no produce un VSIX. Con la CLI ya compilada, ejecuta desde la raíz:

```text
node --test vscode-pypic/test/*.test.js
```

Para abrir la extensión en una ventana de desarrollo, instalarla y configurar la ruta de la CLI y los Device Packs, sigue [la guía de la extensión](../vscode-pypic/README.md). La extensión actual permite comprobar, compilar y abrir ejemplos; no programa el PIC ni implementa depuración física.

## Problemas frecuentes

- **`cargo` no se reconoce:** instala Rust y abre una terminal nueva; confirma con `cargo --version`.
- **No se encuentra el Device Pack:** ejecuta desde la raíz o indica `--devices-dir` / `PYPIC_DEVICES`.
- **La extensión no encuentra `pypic`:** compila primero la CLI y configura `pypic.executable` con la ruta absoluta al ejecutable.
- **Las pruebas de PIC-AS no corren:** son opcionales y necesitan las dos variables y herramientas descritas en la guía del ensamblador.
