# Pinout en la extensión de VS Code

La sección persistente **Pinout** de la barra lateral PyPIC muestra el encapsulado del dispositivo activo sin abrir un modal ni ocupar una pestaña del editor. La orden **PyPIC: Ver pinout** enfoca esa misma sección. La vista lee `device.json` y `pins.json` del Device Pack configurado; no mantiene una segunda tabla de pines.

## Comportamiento

- Si el archivo activo declara un PIC, la vista carga ese dispositivo.
- Sin declaración en el fuente, usa el dispositivo del perfil seleccionado, el perfil predeterminado o el único perfil de `pypic.toml`, en ese orden.
- **Visualizar PIC** permanece disponible y se alimenta con todos los Device Packs válidos de la carpeta configurada. **Automático** sigue el fuente o perfil; elegir otro modelo se recuerda como preferencia de visualización y no modifica el proyecto.
- El diagrama usa filas compactas para mostrar más pines sin perder la selección por teclado ni el detalle inferior.
- Cada pin muestra su puerto (PORTA, PORTB, PORTC, PORTD o PORTE) y conserva un tooltip con el puerto, nombre GPIO y funciones alternativas.
- **Encapsulado** permite cambiar entre los paquetes definidos por el Device Pack. El PIC16F887 incluido actualmente declara PDIP40; un QFN/TQFP-44 aparecerá automáticamente cuando se agregue su mapa oficial de pines.
- El PDIP se orienta con el pin 1 arriba a la izquierda; el lado derecho empieza con el último pin arriba.
- Cada pin muestra su nombre GPIO, número físico y funciones alternativas. Al seleccionarlo se expande el detalle de funciones y tipo.
- Los colores distinguen GPIO digital, pin con función analógica, alimentación y pin de solo entrada; el texto y la leyenda conservan la información sin depender del color.
- Al seleccionar un pin, el detalle fijo al borde inferior muestra funciones, **registros** (`PORTx<n>`, `TRISx<n>`, `WPUB<n>`, `IOCB<n>`, canal analógico), **corriente** (25 mA máximos absolutos por pin; 95 mA por `VDD`/`VSS` en total), **niveles** (`VOL` ≤ 0,6 V a 8,5 mA, `VOH` ≥ VDD − 0,7 V a 3 mA, fuga ±1 µA), **pull-up** (solo PORTB: `WPUB<n>` = 1 con `RBPU` = 0, 50 a 400 µA; se apaga si el pin es salida o analógico) y la línea de importación de PyPIC.
- Esos datos salen del bloque `electrical` de `pins.json` y de los campos `weak_pullup` e `interrupt_on_change` de cada pin; el Device Pack valida que los bits existan en `registers.json`. Fuente: DS40001291H pp. 239 y 245-246 (límites absolutos y características de E/S) y p. 50 (registro WPUB). Los 25 mA son un límite de daño, no un valor de uso continuo.
- Si falta el pack, la vista explica la causa y abre la selección de Device Packs.

## Criterios de aceptación

1. El PIC16F887 muestra 40 pines en el orden físico del PDIP-40.
2. RD0 aparece como pin 19; RB7/ICSPDAT como pin 40; VDD y VSS se distinguen como alimentación.
3. Todos los pines se recorren con teclado y tienen un nombre accesible que incluye número, GPIO y funciones.
4. El panel funciona con temas claros y oscuros de VS Code y conserva el diagrama a ancho estrecho.
5. Un Device Pack ausente produce una recuperación visible, no una sección vacía.
6. Al instalar packs de otras familias, el catálogo los descubre sin cambiar el código de la extensión.
7. El selector permite alternar entre **Automático** y cualquier modelo instalado sin cambiar el objetivo de compilación.

Fuente: Microchip `DS40001291H`, página 7, “Pin Diagrams – PIC16F884/887, 40-Pin PDIP”.
