# Device Packs (`pypic-device`)

**Estado: implementado (primer corte, esquema v1).** Un Device Pack describe un MCU con datos verificados contra su
datasheet oficial. Todo corre en el host; el MCU solo ejecuta el firmware nativo resultante.

```
devices/<mcu>/
  device.json   identidad, CPU (data_bits, instrucciones, reloj), E/S, periféricos, nivel de soporte y max_int_bits
  memory.json   programa (palabras, vectores, pila), datos (bancos, SFR por banco, GPR, zona común) y EEPROM
  pins.json     encapsulados con número de pin, puerto/bit y funciones alternas
  config.json   palabras de configuración (dirección, valor por defecto, campos y significado de cada valor)
```

Pendiente (issues abiertos): `registers.json` (mapa SFR con bits), `peripherals.json`, más encapsulados y PIC12F625.

## Reglas del esquema

- Cada archivo lleva `schema_version` (hoy `1`). El cargador **rechaza campos desconocidos** y versiones que no conoce.
- Direcciones y valores numéricos pueden ser enteros o cadenas `"0x1FFF"`.
- Cada dato lleva su fuente (`source`/`sources`) con página del datasheet.
- `cpu.data_bits` es el ancho nativo del MCU (8/16/32) y `pypic.max_int_bits` el ancho máximo de entero que PyPIC admite
  (nativo o emulado). De ahí sale el `TargetProfile` del frontend: `native_bits = cpu.data_bits`,
  `max_int_bits = pypic.max_int_bits`. Los tipos más anchos que el nativo compilan con aviso de coste (`PYP3001`).
- `pypic.support_level`: `experimental`, `compila`, `simula`, `probado en hardware`, `depuracion fisica`.

## Validación (`DevicePack::validate`, `pypic device validate <dir>`)

Comprueba la coherencia interna, no solo el formato:

- **Memoria:** vectores dentro del programa y reset anterior a interrupción; cada banco cubierto **exactamente** por
  SFR + GPR (+ espejo de la zona común), sin huecos ni solapes; suma de GPR = `ram_bytes`; espejos coherentes.
- **Pines:** números únicos y dentro de `pin_count`; nombre coherente con puerto/bit (`RB3` = puerto B, bit 3);
  presencia de VDD/VSS; cantidad de pines de E/S (sin contar los solo-entrada) = `io_pins`.
- **Configuración:** direcciones dentro del espacio de configuración y sin repetir; campos dentro de los 14 bits y sin
  solaparse; cada campo define todos sus valores posibles; `default` cabe en la palabra.
- **Dispositivo:** `data_bits` y `max_int_bits` válidos (8/16/32, `max_int_bits >= data_bits`), nivel de soporte
  conocido y al menos una fuente.

## Uso

```
cargo run -p pypic-cli -- device validate devices/pic16f887
cargo run -p pypic-cli -- check examples/counter.py --device devices/pic16f887
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
