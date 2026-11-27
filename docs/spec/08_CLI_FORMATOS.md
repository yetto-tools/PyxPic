# CLI, estructura de proyecto y contratos

CLI propuesto: pyxpic init; pyxpic check; pyxpic build --target pic16f887 --toolchain native; pyxpic build --toolchain xc8; pyxpic sim; pyxpic flash; pyxpic debug; pyxpic device list/validate; pyxpic migrate --to PIC12F625 --dry-run; pyxpic tools i2c-scan; pyxpic tools serial. Los comandos son especificación propuesta, no implementados.

Estructura sugerida: compiler/frontend, compiler/ir, compiler/passes, backends/pic14-native, backends/c-xc8, backends/asm-picas, devices, sdk, simulator, debug, cli, vscode-extension, tests, examples, docs. Artefactos: .hex, .asm, .c, .map, .pyxpicdbg, report.json, build manifest. Versionar ABI y schema de packs.

Ejemplo pyxpic.toml:
```toml
[project]
name = "MiProyecto"
version = "0.1.0"
[target]
device = "PIC16F887"
package = "DIP40"
clock = 8000000
[toolchain]
provider = "native"
optimization = "size"
[debug]
enabled = true
backend = "simulator"
[programmer]
provider = "auto"
```
