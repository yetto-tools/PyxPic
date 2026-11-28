# Device Packs (`pyxpic-device`)

**Estado: implementado.** Un Device Pack describe un MCU con datos verificados contra su
datasheet oficial. Todo corre en el host; el MCU solo ejecuta el firmware nativo resultante.

```
devices/<mcu>/
  device.json   identidad, CPU (data_bits, instrucciones, reloj), E/S, periféricos, nivel de soporte y max_int_bits
  memory.json   programa (palabras, vectores, pila), datos (bancos, SFR por banco, GPR, zona común) y EEPROM
  pins.json     encapsulados con número de pin, puerto/bit y funciones alternas
  config.json   palabras de configuración (dirección, valor por defecto, campos y significado de cada valor)
  registers.json mapa SFR: nombre, direcciones en cada banco, nombre de cada bit, valor tras POR y página del datasheet
  peripherals.json periféricos: registros, bits de encendido, interrupciones, funciones de pin y dependencias
```

Pendiente (issues abiertos): más encapsulados (#24) y PIC12F625 (#25).

## Reglas del esquema

- Cada archivo lleva `schema_version`: `1`, salvo `config.json`, que también admite la versión `2` con ubicaciones de identificación opcionales. Los packs v1 siguen siendo válidos. El cargador **rechaza campos desconocidos** y versiones que no conoce.
- En `config.json` v2, `id_locations` describe nombre, dirección y máscara de bits útiles, separados de los campos de configuración. En el PIC16F887 son `IDLOC0`–`IDLOC3`, direcciones `2000h`–`2003h` y máscara `7Fh` (datasheet, sección 14.8).
- Direcciones y valores numéricos pueden ser enteros o cadenas `"0x1FFF"`.
- Cada dato lleva su fuente (`source`/`sources`) con página del datasheet.
- `cpu.data_bits` es el ancho nativo del MCU (8/16/32) y `pyxpic.max_int_bits` el ancho máximo de entero que PyxPic admite
  (nativo o emulado). De ahí sale el `TargetProfile` del frontend: `native_bits = cpu.data_bits`,
  `max_int_bits = pyxpic.max_int_bits`. Los tipos más anchos que el nativo compilan con aviso de coste (`PYP3001`).
- `pyxpic.support_level`: `experimental`, `compila`, `simula`, `probado en hardware`, `depuracion fisica`.

## Validación (`DevicePack::validate`, `pyxpic device validate <dir>`)

Comprueba la coherencia interna, no solo el formato:

- **Memoria:** vectores dentro del programa y reset anterior a interrupción; cada banco cubierto **exactamente** por
  SFR + GPR (+ espejo de la zona común), sin huecos ni solapes; suma de GPR = `ram_bytes`; espejos coherentes.
- **Pines:** números únicos y dentro de `pin_count`; nombre coherente con puerto/bit (`RB3` = puerto B, bit 3);
  presencia de VDD/VSS; cantidad de pines de E/S (sin contar los solo-entrada) = `io_pins`.
- **Registros:** cada dirección de la zona SFR de `memory.json` tiene un registro o está declarada sin implementar; ninguna dirección
  la comparten dos registros salvo el par con condición de selección (SSPADD/SSPMSK en `93h`); nombres de registro y de bit únicos; 8 bits por registro;
  un registro conserva el mismo desplazamiento en todos los bancos; los bits RP0/RP1 de STATUS existen; cada pin tiene su bit en `PORTx` y `TRISx`
  y cada `ANSn` su bit en ANSEL/ANSELH.
- **Periféricos:** los registros y bits (encendido, banderas y habilitaciones de interrupción) que cita cada uno existen en `registers.json`; cada función
  alterna de `pins.json` la posee **exactamente un** periferico y cada función de un periférico está en algún pin; los modos que usan los pines están declarados;
  `uses` apunta a periféricos que existen; y las cantidades coinciden con `device.json` (temporizadores, canales y bits del ADC, comparadores, CCP/ECCP/EUSART/MSSP).
- **Configuración:** direcciones de configuración e identificación dentro del espacio de configuración y sin repetir; máscaras de identificación válidas; campos dentro de los 14 bits y sin
  solaparse; cada campo define todos sus valores posibles; `default` cabe en la palabra.
- **Dispositivo:** `data_bits` y `max_int_bits` válidos (8/16/32, `max_int_bits >= data_bits`), nivel de soporte
  conocido y al menos una fuente.

## Uso

```
cargo run -p pyxpic-cli -- device validate devices/pic16f887
cargo run -p pyxpic-cli -- check examples/python/gpio/counter.py --device devices/pic16f887
```

`--native-bits` y `--max-int-bits` sobrescriben los valores del pack.

## PIC16F887 (`devices/pic16f887`)

Fuente: **DS40001291H** (2006-2015), <https://ww1.microchip.com/downloads/aemDocuments/documents/OTH/ProductDocuments/DataSheets/40001291H.pdf>
(SHA-256 de la copia usada: `8cca4a12d17d06af…`, 4 195 787 bytes).

| Dato | Valor | Página |
|---|---|---|
| Programa | 8192 × 14 bits, reset `0000h`, interrupción `0004h`, pila de 8 niveles | 22 |
| Datos | 4 bancos × 128 B; selección con RP0/RP1 de STATUS | 23 |
| RAM de propósito general | 20h–7Fh, A0h–EFh, 110h–16Fh, 190h–1EFh = **368 B**; zona común 70h–7Fh reflejada en F0h/170h/1F0h | 23, 26 |
| SFR por banco | 00h–1Fh, 80h–9Fh, 100h–10Fh, 180h–18Fh | 26 |
| Configuración | CONFIG1 en `2007h`, CONFIG2 en `2008h`, ambas `3FFFh` sin programar; espacio `2000h–3FFFh` | 206–207 |
| Pines | PDIP-40, 35 de E/S (RE3 es solo entrada, MCLR/VPP) | 7 |

**Hallazgo:** el texto general de la sección 2.2 dice que los SFR ocupan las 32 primeras posiciones de cada banco, pero la
figura 2-6 de la F887 muestra que en los bancos 2 y 3 son solo 16 y que hay 16 bytes extra de GPR (110h–11Fh y 190h–19Fh).
Sin ellos la RAM sumaría 336 B, no 368. Por eso el esquema describe el rango SFR de **cada banco**.

**Limitaciones conocidas:** la tabla de asignación de pines (pág. 8) no se pudo extraer de forma fiable, así que el mapa de
pines sale del diagrama de la pág. 7. Falta contrastar el pack con el datasheet del F887 en encapsulados QFN/TQFP de 44 pines.

### Registros (`registers.json`)

73 registros de las Tablas 2-1 a 2-4 (p.27-29). `bits` lista el nombre del bit 0 al 7 (`null` = sin nombre o no implementado). `physical: false` marca INDF y EECON2.
Los nombres de bit dudosos del datasheet se escriben como identificadores (`GO/DONE` → `GO_DONE`, `D/A` → `D_A`, `R/W` → `R_W`).

El ensamblador en línea ([asm-inline.md](asm-inline.md)) toma de aquí todos los nombres: cada registro vale su **desplazamiento de 7 bits** (`TRISD` = `0x08`; el banco
lo elige el código con `RP0`/`RP1`) y cada bit su número (`RP0` = 5, `GO_DONE` = 1). Un nombre de parámetro o global solo choca si el bloque lo usa.

### Periféricos (`peripherals.json`)

17 entradas con la página de su capítulo: `TIMER0`, `TIMER1`, `TIMER2`, `ADC`, `COMPARATORS`, `VREF`, `EEPROM`, `ECCP` (CCP1), `CCP2`, `EUSART`, `MSSP`, `OSCILLATOR`, `ULPWU`,
`EXT_INT`, `PORTB_IOC`, `WDT` y `PROGRAMMING` (MCLR/ICSP). Cada una dice qué registros la controlan, qué bits la encienden, sus pares bandera/habilitación de interrupción,
las funciones de pin que posee (con dirección `in`/`out`/`io`/`analog`) y de qué otros periféricos depende (`ECCP` y `CCP2` usan Timer1 y Timer2). `MSSP` declara los modos
`SPI` e `I2C`, que se excluyen y se reparten los pines (`SCK`/`SDI`/`SDO`/`SS` frente a `SCL`/`SDA`).

`DevicePack::pin_uses("RA4")` devuelve lo que un pin cedería a periféricos: `T0CKI:TIMER0` y `C1OUT:COMPARATORS`. Es la base de los avisos de conflicto de pines
y de que el SDK de periféricos ([sdk-modulos.md](sdk-modulos.md)) sepa qué pines reclama cada módulo.

## PIC16F628A (`devices/pic16f628a`)

Fuente: **DS40044G** (2009), <https://ww1.microchip.com/downloads/en/DeviceDoc/40044G.pdf> (SHA-256 de la copia usada: `adb957497df5779f…`).

| Dato | Valor | Página |
|---|---|---|
| Programa | 2048 x 14 bits, reset `0000h`, interrupción `0004h`, pila de 8 niveles | 17 |
| RAM | 20h-7Fh, A0h-EFh, 120h-14Fh = **224 B**; 150h-16Fh y 1A0h-1EFh sin implementar | 18 |
| EEPROM | 128 B | 3 |
| Configuración | una palabra en `2007h`; `FOSC<2:0>` en los bits 4, 1 y 0 | 98 |
| Pines | PDIP-18, 15 de E/S (RA5 es solo entrada, MCLR/VPP) | 4 |
| Reloj | INTOSC 4 MHz; LP hasta 200 kHz, XT 0,1-4 MHz, HS 1-20 MHz | 99, 144 |

**Estado: experimental.** El backend genera GPIO, esperas, registros, ensamblador en línea y el lenguaje. Los periféricos del SDK
(temporizadores, PWM, UART, EEPROM, comparadores...) dan error hasta tener sus pruebas. Se probó en el simulador (que modela el PIC16F887:
solo se usa el núcleo y PORTB/TRISB, que coinciden). **En hardware (2026-11-28)** solo se ha probado `examples/python/pic16f628a/blink.py`:
el LED de RB0 parpadea con el reloj interno. El resto no está probado en hardware.

- No hay `ANSEL`: al configurar RA0-RA3 se escribe `CMCON = 07h`, que **apaga los dos comparadores** (p.63-64).
- **RA4 es de drenador abierto** (p.33, p.37): como salida no pone nivel alto; un LED va entre VDD y el pin.
- Palabra por defecto de `pyxpic build`: `0x3F18` (INTOSC con E/S en RA6/RA7, WDT, PWRT, BOR, LVP y protecciones apagados, MCLR interno).
- Del oscilador interno solo se admiten 4 MHz; el de 48 kHz (`PCON<OSCF>`) no.
- Falta: límites eléctricos de los pines y los encapsulados SSOP y QFN.

Esquemas ampliados para este chip: `config.json` versión 3 (`extra_bits`, campos de bits no contiguos) y `memory.json` versión 2
(`data.unimplemented`, huecos de un banco). Las versiones anteriores siguen siendo válidas.

Criterios de aceptación (`crates/pyxpic-pic14/tests/pic16f628a.rs`): el pack valida; el ejemplo conmuta RB0 cada 500 ms en el simulador;
un pin inexistente o un periférico sin soporte son errores.

## Configuración por nombre (`config.json`)

Cada palabra tiene `default` (el valor de la memoria sin programar, `0x3FFF`) y, opcionalmente, **`build_default`**: lo que `pyxpic build` programa si no se pide otra cosa
(en el PIC16F887, `CONFIG1 = 0x20D4`: oscilador interno INTOSCIO, WDT apagado, MCLR interno). Cada campo puede traer **`names`**: el nombre del valor en pic-as y en el DFP
(`"ON"`, `"OFF"`, `"INTRC_NOCLKOUT"`...) → valor numérico; el pack comprueba que cada nombre apunte a un valor que exista y que no se repita.

```
pyxpic build blink.py --config WDTE=ON,FOSC=XT --config BOR4V=BOR21V     # por nombre, repetible
pyxpic build blink.py --config CONFIG1=0x20D4                            # o la palabra entera
```

Se parte de `build_default` y se aplica cada ajuste encima; un ajuste o valor que el pack no tiene es un error que lista las opciones (`ON (WDT habilitado), OFF (...)`),
y repetir un ajuste también. El mismo `config.json` da los nombres de `CONFIG` en `pyxpic asm` ([asm-pic14.md](asm-pic14.md)). Los límites de memoria
(`memory.json`) ya se toman del pack. El archivo de proyecto `pyxpic.toml` para fijar estos ajustes sin la línea de órdenes es #37.
