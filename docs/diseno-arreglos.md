# Diseño pendiente: arreglos estáticos en RAM

**Estado: la primera entrega está implementada (ver `docs/sdk-arreglos.md`); este documento conserva el diseño y lo que sigue pendiente.** Lo siguiente, escrito antes de implementar, queda como contexto: El objetivo es habilitar búferes explícitos para recepción y formato de texto (#63), sin reservar un búfer implícito para `UART.write_text` ni introducir un intérprete en el PIC.

## Primera entrega propuesta

Admitir una declaración de capacidad fija y elementos `uint8`, por ejemplo `buf: uint8[16]`, y lectura y escritura mediante `buf[i]`. La capacidad debe ser conocida en compilación y positiva. Cada elemento ocupa un byte de RAM; `uint16`, cadenas y arreglos de otros tipos requieren trabajo adicional y no deben aceptarse como si estuvieran implementados. Los arreglos en ROM (`Rom[uint8]`) conservan su contrato existente y son independientes.

Antes de habilitar la sintaxis debe definirse expresamente si se permite declarar arreglos locales, cómo se inicializan y qué sucede cuando un índice variable queda fuera de rango. No asumir que el comportamiento de las tablas ROM se aplica a la RAM: una escritura fuera de rango no debe modificar otras variables o registros. El compilador debe rechazar los índices constantes fuera de rango y los tamaños que no quepan en la RAM disponible.

## Cambios necesarios

- **Frontend:** analizar declaración, lectura y asignación indexada; comprobar tipo, capacidad e índices constantes; emitir diagnósticos con ubicación.
- **IR e intérprete:** representar el almacenamiento mutable y las operaciones de lectura y escritura; definir el comportamiento de los índices variables y la inicialización. Validar las referencias y los tipos en el verificador IR.
- **PIC14:** reservar RAM de forma explícita y contabilizar capacidad, temporales y acceso indirecto. El backend actual utiliza únicamente el banco 0 (`20h–6Fh`, 80 bytes compartidos por globales, locales y temporales); no prometer que un arreglo de 80 bytes cabrá en cualquier programa. Dar error si no cabe.
- **Interrupciones:** preservar FSR y el estado de banco necesarios cuando el código principal y una ISR utilicen acceso indirecto. Probar una interrupción durante un acceso al arreglo y respetar el ABI documentado.
- **Pruebas y documentación:** contrastar intérprete IR y simulador PIC14 para lectura, escritura, límites y RAM agotada; documentar sintaxis, inicialización, costo y restricciones. Probar cualquier ejemplo de recepción por separado antes de declararlo funcional.

`char[N]`, recepción de líneas, conversión de texto a entero y `format_into` son funcionalidades posteriores de #63; esta primera entrega no las implementa por sí sola.
