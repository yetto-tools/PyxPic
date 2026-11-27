# Terminal serial: `pyxpic monitor` (#72)

Lee un puerto serie (o la salida por UART de un programa en el simulador) y la muestra; lo que se escribe en la entrada estándar se envía al puerto. Todo es local: no se envía nada fuera del equipo.

```
pyxpic monitor --list [--format json]                  puertos disponibles
pyxpic monitor --port COM3 --baud 9600 [opciones]      terminal serial
pyxpic monitor --sim programa.py|.bas [--seconds N]    la UART del programa en el simulador (sin hardware)
```

Opciones: `--data-bits 5..8`, `--parity none|even|odd`, `--stop-bits 1|2`, `--flow none|rts|xon`, `--encoding ascii|utf8|hex`, `--timestamps` (`[MM:SS.mmm]` desde que empezó la sesión), `--log archivo`, `--filter REGEX`, `--format text|jsonl`, `--eol none|cr|lf|crlf`, `--send-encoding ascii|hex`, `--send-file archivo`, `--reconnect`. Con `--sim` también valen `--devices-dir`, `--project`, `--target`, `--device`, `--config` y `--no-opt`, como en `build`.

## Errores

| Situación | Salida |
|---|---|
| Puerto inexistente | mensaje con el nombre y la pista `pyxpic monitor --list`, código 2 |
| Puerto ocupado o sin permiso | mensaje que lo dice, código 2 |
| Configuración no admitida | mensaje que sugiere otra velocidad u otros bits, código 2 |
| Desconexión a mitad de sesión | evento `closed`, código 3 (con `--reconnect` reintenta cada segundo) |

## Formato `--format jsonl`

Una línea JSON por evento, con `"version": 1`; agregar campos no sube la versión.

- `{"type":"data","t_ms":…,"hex":"48 6F","text":"Ho","bytes":2}`
- `{"type":"open"|"closed"|"waiting","message":"…"}`

`--list --format json` devuelve `{"version":1,"ports":[{"name","type", y vid/pid/producto en USB}]}`.

## Límites

- Sin UART por software todavía (#47): hace falta el puerto de hardware (RC6/RC7 en el PIC16F887).
- `--sim` reproduce lo que el programa envía; no modela un dispositivo conectado al RX.
- Gráficas, escáner I²C y decodificador de protocolos son entregas posteriores.

Dependencia: crate `serialport` 4.10 (licencia MPL-2.0).
