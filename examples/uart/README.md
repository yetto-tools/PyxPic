# Ejemplos del puerto serie (EUSART del PIC16F887)

Todos usan la API real `pypic.periph.uart` (`docs/sdk-uart.md`), en modo asíncrono 8N1 a 9600 baudios con `CLOCK_HZ` = 4 MHz (el compilador elige `BRGH` = 1 y `SPBRG` = 25: 9615 baudios, +0,16 %).

## Conexión mínima

| PIC16F887 | Adaptador USB-UART (3,3 V o 5 V, el que tolere el PIC) |
|---|---|
| RC6/TX | RX |
| RC7/RX | TX |
| GND | GND |

- La terminal debe abrirse a la **misma velocidad** que `UART.start(baud=...)`, en 8N1 y sin control de flujo.
- `CLOCK_HZ` debe ser el reloj **real** del PIC (la palabra de configuración que genera `pypic build` elige el oscilador interno por defecto). Si no coincide, los baudios salen mal.
- RS-232 (±12 V) y RS-485 **necesitan un transceptor** (MAX232, MAX485...): no conectes el PIC directamente a esos niveles.

## Compilar y probar

```
pypic build examples/uart/eco.py -o eco.hex --timing    # --timing imprime la velocidad real y su error
```
Los `.hex` ya generados están en `examples/hex/uart_*.hex`. Para verlos funcionar sin hardware, las pruebas del simulador están en `crates/pypic-pic14/tests/uart_examples.rs` (`cargo test -p pypic-pic14 --test uart_examples`).

## Ejemplos

| Archivo | Qué hace | Ante lo inesperado |
|---|---|---|
| `saludo.py` | Envía `Hola PyPIC` + CR LF cada segundo (solo transmite) | No recibe, nada que tratar |
| `eco.py` | Devuelve cada byte sin modificar (incluidos `0x00` y `0xFF`) | Borra `OERR`; los bytes perdidos no se recuperan |
| `led_comandos.py` | `A` enciende RD0, `B` lo apaga | Ignora cualquier otro byte; borra `OERR` |
| `no_bloqueante.py` | Parpadea RD0 mientras consulta la UART, con eco | El parpadeo no se detiene; borra `OERR` |
| `errores.py` | Lee `FERR` antes del byte; recupera `OERR` | Error de trama: descarta y contesta `?`; desbordamiento: contesta `O` |
| `gps_nmea.py` | Reconoce tramas NMEA, comprueba su checksum y cuenta | Resincroniza en el siguiente `$`; checksum malo = inválida |
| `direccion_9bits.py` | Direccionamiento multipunto con 9 bits: bit 8 = 1 es dirección, 0 es dato | Datos sin atender se ignoran; borra `OERR` |

`examples/uart_echo.py` (fuera de esta carpeta) devuelve cada byte **incrementado en uno**; no es el eco.

## Verificación: qué está probado y qué no

- **Simulador** (`uart_examples.rs`): cada ejemplo compila para el PIC16F887 y se prueba con entrada inyectada: orden y contenido de los bytes, el LED, el error de trama, el desbordamiento y, en el GPS, checksum válido, inválido y trama truncada.
- **Proteus** (terminal virtual, PIC a 4 MHz): `saludo.hex` transmite bien y `led_comandos.hex` recibe: `A` enciende el LED y `B` lo apaga; `no_bloqueante.hex` parpadea y hace eco a la vez. En las propiedades del PIC hay que poner **Processor Clock Frequency = el `CLOCK_HZ` del programa** (por defecto Proteus usa 1 MHz y la terminal muestra basura).
- `direccion_9bits.py` solo está verificado en el simulador: **no se ha probado en Proteus ni en hardware**.
- **Hardware real: ninguno.** No se ha probado con un adaptador ni con un GPS. El simulador usa la misma tabla de baudios que el compilador, así que no ve la desviación del oscilador interno ni los niveles eléctricos.
- `gps_nmea.py` **no es un analizador GPS**: solo valida y cuenta tramas.

## Lo que falta (todavía no está implementado)

Interrupciones con ayudas propias, `ADDEN` (filtro de direcciones por hardware), autobaudios y ruptura (LIN), y modo síncrono: ver `docs/sdk-uart.md`. Los ejemplos de protocolos y dispositivos (HC-05, RS-485/Modbus RTU, RS-232, SIM800L, DFPlayer, Nextion, LIN) y una UART por software (`pypic.soft`) dependen de eso y **no se han escrito**; no se presentan como disponibles.
