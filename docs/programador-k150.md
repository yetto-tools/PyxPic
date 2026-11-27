# Grabar el PIC16F887 con un programador K150 (MicroBrn)

**Estado:** el 2026-11-25 el usuario confirmó en hardware real que un HEX generado por `pyxpic build` con `MCLRE=ON`,
grabado con un K150 y MicroBrn, hace parpadear un LED en el PIC16F887. **No se midieron voltajes** y los tiempos solo
se midieron a mano (ver abajo).
Es un resultado de un solo programador y un solo montaje: no garantiza que funcione con otros K150.

> **Soporte no oficial.** El `readme_ep.pdf` que acompaña a MicroBrn (2007) dice que el firmware P18A **no soporta los
> 16F88x**, por la segunda palabra de configuración y un juego de comandos de programación distinto. Lo que sigue es lo
> que funcionó en la práctica. Para un soporte oficial hace falta un programador que liste el PIC16F887.

PyxPic no depende de MicroBrn ni incluye sus archivos. El programador y su software son de terceros: no se añaden al
repositorio.

## Qué está probado

| Caso | Resultado en hardware |
|---|---|
| `pyxpic build` + `--config MCLRE=ON`, salida directa (finales `LF`), LED en RD0 | Funciona |
| El mismo HEX con finales `CR+LF` | Funciona |
| HEX de PyxPic sin la línea de CONFIG2, LED en RD0 y en RB0 | Funciona |
| HEX de XC8 con `MCLRE=ON`, LED en RB0 | Funciona |
| `MCLRE=OFF` (valor por defecto de PyxPic) | **Sin probar** |
| CONFIG2 distinta de `0x3FFF` (`WRT`, `BOR4V`) | **Sin probar** |
| Datos iniciales de EEPROM en el HEX, grabados por el K150 | Funciona (`0x07` en la dirección 0, leído por el programa) |
| `EEPROM.write` desde el programa: el valor sobrevive al apagado | Funciona (contador 9, 10, 11 en encendidos sucesivos) |
| `UART.write_text` a 9600 8N1 desde RC6/TX hacia un adaptador USB CP210x | Funciona |
| Recepción en RC7/RX (`rx_ready`/`recv`) con comandos y eco desde PuTTY, 9600 8N1 | Funciona (respuestas coherentes en pantalla) |
| PIC alimentado a 3,3 V desde el adaptador, INTOSC a 4 MHz, sin brown-out | Funciona |
| **Blank → Erase Chip** borra la EEPROM | Funciona: `07` leído antes, `FF` leído después, con MicroBrn reiniciado antes de la primera lectura |

Incluir CONFIG2 en el HEX no impide la grabación. Eso **no** demuestra que MicroBrn la grabe: `0x3FFF` es el mismo
valor que deja un borrado.

### Período del parpadeo, medido a mano

`examples/python/gpio/blink.py` (500 ms encendido y 500 ms apagado, período esperado 1,000 s), con el cronómetro de un teléfono y una
marca por encendido: 23 marcas en 23,13 s (1,006 s de media); las últimas 8 visibles, 0,970 s de media. Las vueltas
sueltas van de 0,92 a 1,02 s. Es compatible con lo esperado con un margen de unos pocos puntos porcentuales; **no**
verifica la exactitud al ciclo, que requiere osciloscopio o analizador lógico.

## Programas de prueba (`examples/python/hardware/`)

Todos se compilan con `--config MCLRE=ON`.

| Programa | Qué hace |
|---|---|
| `blink_rb0.py` | Parpadeo de 500 ms en RB0 (pin 33) |
| `blink_rapido.py` | Parpadeo de 100 ms en RD0 (pin 19); preparado para la prueba pendiente de `MCLRE=OFF`, compilándolo **sin** `--config` |
| `eeprom_lee.py` | Parpadea en RD0 tantas veces como vale el byte 0 de la EEPROM; solo lee |
| `serial_eeprom.py` | Envía cada segundo el byte 0 de la EEPROM por RC6/TX y cambia RD0 |
| `serial_comandos.py` | Comandos `1`, `0`, `t`, `?` y eco por la UART |

PyxPic todavía no declara datos iniciales de EEPROM desde el `.py`. Para la prueba se añadió al HEX, antes del registro
final, `:024200000700B5` (valor `0x07` en la dirección 0).

**Criterio de aceptación** de cada programa en hardware: se compila sin errores, MicroBrn termina **Program** y
**Verify** sin errores, y se observa el comportamiento de la tabla.

## Montaje

| Pin (PDIP-40) | Conexión |
|---|---|
| 1 (RE3/MCLR) | 10 kΩ a +5 V (necesaria con `MCLRE=ON`) |
| 11 y 32 (VDD) | +5 V |
| 12 y 31 (VSS) | GND |
| El del programa | resistencia de 330 Ω → LED → GND |

**Comprueba qué pin usa cada programa** antes de decidir que algo falló:

| Programa | Pin del LED |
|---|---|
| `examples/python/gpio/blink.py` | RD0, pin 19 |
| Un blink sobre `RB0` | RB0, pin 33 |

## Salida serie

Para ver datos sin contar parpadeos: `UART.start(baud=9600)` y un adaptador TTL-USB.

| PIC16F887 | Adaptador |
|---|---|
| Pin 25 (RC6/TX) | RXD |
| Pin 26 (RC7/RX) | TXD (directo solo con el PIC a 3,3 V) |
| Pines 12 y 31 | GND |
| Pines 11 y 32 | 3V3 (solo si el adaptador alimenta el PIC) |

- Con el PIC a **3,3 V** la señal va directa a RXD. Con el PIC a **5 V**, comprueba que la entrada del adaptador lo admita o
  usa un divisor (1 kΩ en serie y 2 kΩ a GND).
- Con el PIC a 3,3 V, la resistencia de 10 kΩ del pin 1 va a 3,3 V.
- En la primera conexión el USB del adaptador se desconectó (probable corto o exceso de consumo en 3V3): revisa el
  cableado de alimentación sin tensión. Los pines 11-12 son VDD-VSS y los 31-32 son VSS-VDD (orden invertido).

## Pasos

1. Compilar desde la raíz del repositorio:

   ```
   cargo run -p pyxpic-cli -- build examples/python/gpio/blink.py --config MCLRE=ON -o blink.hex
   ```

   Sin `-o` o sin revisar la ruta se puede cargar por error un HEX antiguo.
2. En MicroBrn, elegir el programador `K150`, el puerto y el chip `16F887`.
3. Cargar el HEX y revisar los fusibles: oscilador `INTRC_IO`, MCLRE habilitado, LVP y WDT deshabilitados.
4. **Erase**, **Program** y **Verify**.
5. Pasar el chip al montaje y comprobar el programa.

**Borrar sin grabar:** el botón **Blank** abre un cuadro con *Erase Check* (solo comprueba) y *Erase Chip* (borra). Al pulsar
**Program** sobre un chip con datos, MicroBrn avisa y pide permiso para borrarlo.

**Tamaño de los archivos de Save:** parece omitir las zonas en blanco del final. La lectura de un chip borrado se guardó en
30 bytes, solo con CONFIG1 = `0x3FFF`. Por eso un archivo con menos de 8192 palabras no indica por sí solo una lectura
incompleta.

Antes de borrar un chip que ya tiene un programa que interese, léelo (**Read**) y guárdalo. Una lectura guardada no
garantiza que se pueda restaurar: compara dos lecturas y revisa en pantalla la memoria completa y la EEPROM.

## Entrada del 16F887 en `chipdata.cid`

La distribución de MicroBrn usada no traía el 16F887. Se añadió a mano a partir de la entrada del 16F917. Haz una copia
de `chipdata.cid` antes de editarlo, y con MicroBrn cerrado.

```
CHIPname=16F887
INCLUDE=Y
KITSRUS.COM=40pin
EraseMode=2
FlashChip=Y
PowerSequence=VccVpp1
ProgramDelay=50
ProgramFlag2=1
PanelSizing=0
CoreType=bit14_B
ROMsize=002000
EEPROMsize=00000100
FUSEblank=3FFF
CPwarn=N
CALword=N
BandGap=N
ICSPonly=N
ChipID=2080
LIST1 FUSE1 "WDT" "Enabled"=3FFF "Disabled"=3FF7
LIST2 FUSE1 "PWRTE" "Disabled"=3FFF "Enabled"=3FEF
LIST3 FUSE1 "BODEN" "BODon SBODENoff"=3FFF "BODrun SBODENoff"=3EFF "SBODEN=PCON"=3DFF "BODoff SBODENoff"=3CFF
LIST4 FUSE1 "InEx Switch Over" "Enabled"=3FFF "Disabled"=3BFF
LIST5 FUSE1 "Fail Clock Mon" "Enabled"=3FFF "Disabled"=37FF
LIST6 FUSE1 "CPD" "Disabled"=3FFF "Enabled"=3F7F
LIST7 FUSE1 "MCLRE" "Disabled"=3FDF "Enabled"=3FFF
LIST8 FUSE1 "LVP" "Enabled"=3FFF "Disabled"=2FFF
LIST9 FUSE1 "DEBUG" "Disabled"=3FFF "Enabled"=1FFF
LIST10 FUSE1 "Oscillator" "EXTRC_CLKOUT"=3FFF "EXTRC_IO"=3FFE "INTRC_CLKOUT"=3FFD "INTRC_IO"=3FFC "EC"=3FFB "HS"=3FFA "XT"=3FF9 "LP"=3FF8
LIST11 FUSE1 "Code Protect" "Disabled"=3FFF "Enabled"=3FBF
```

Las opciones de CONFIG1 coinciden con `devices/pic16f887/config.json`, y el ID `0x2080` con DS41287D. Solo hay una
palabra de fusibles (`FUSEblank=3FFF`): CONFIG2 no se puede ver ni editar desde la pantalla.

### Secuencia de encendido y `MCLRE=OFF`

La especificación de programación **DS41287D, sección 3.0**, exige entrar en modo programación con **VPP antes que VDD**
cuando se usa INTOSC con `MCLRE=0`, que es la configuración por defecto de PyxPic (`CONFIG1=0x20D4`). `VccVpp1` aplica
VDD primero. Por eso, mientras no se pruebe otra cosa, compila con `--config MCLRE=ON`: con `MCLRE=OFF` el chip podría
no dejarse volver a grabar o borrar con este programador.

- Un retardo al inicio del programa no sustituye lo que exige la especificación.
- `Vpp2Vcc` **no** es la solución: en la base de chips usada, ningún dispositivo de 40 pines emplea `Vpp2`.
- `Vpp1Vcc` sería el candidato para VPP primero; ninguna entrada lo usa y no se ha probado.

## Avisos

- Los `epk*.hex` son firmware del **propio programador** (su PIC16F628A). `epk150.hex` es el del K150. No se graban en el
  PIC16F887.
- MicroBrn no lee registros Intel HEX con un número impar de bytes (según su readme). La salida de PyxPic escribe palabras
  completas.
- Según el mismo readme, MicroBrn no guarda bien los fusibles, el ID ni la EEPROM al usar **Save**: no tomes la
  configuración de un archivo guardado como lectura fiable del chip.

## Pendiente

- Probar `MCLRE=OFF`, incluida la regrabación posterior. **No se probó a propósito** (2026-11-25): solo había un PIC16F887
  y ningún programador con VPP primero, así que un fallo habría dejado sin forma de regrabarlo. No hace falta daño en el
  chip para eso: DS41287D indica que la entrada con VPP primero vale con cualquier configuración.
- Probar CONFIG2 distinta del valor borrado.
- Medir los tiempos con instrumento.

## Referencias

- Microchip, *PIC16F88X Memory Programming Specification*, DS41287D.
- Microchip, *PIC16F887 Data Sheet*, DS40001291H.
- [Blink en Proteus](proteus-blink.md).
- Introducción de terceros, **no oficial**:
  [PIC16F887 en microcontrollerslab.com](https://microcontrollerslab.com/pic16f887-microcontroller-pinout-programming-examples-applications-features/)
  (pinout, periféricos y herramientas de programación; en inglés). Sirve para orientarse; ante cualquier duda mandan el
  datasheet y la especificación de programación. Al revisarla (2026-11-25) tenía errores: los números que llama «GPIO»
  conviene verificarlos como pines en el datasheet, mezcla pines de CCP y de oscilador, presenta la RAM y la flash con
  unidades equivocadas o ambiguas y en algún punto nombra otro PIC.
