"""Recepcion no bloqueante: el programa hace otra cosa (parpadea RD0) mientras consulta la UART, y devuelve (eco) lo que llega.

Conexion: RC6/TX -> RX del adaptador, RC7/RX <- TX del adaptador, tierra comun, un LED en RD0. Terminal a 9600 8N1.
El parpadeo no se detiene esperando datos: con UART.read() el programa se quedaria bloqueado; con rx_ready() + recv() no.

Politica ante lo inesperado:
  * Cualquier byte se devuelve tal cual; si no llega nada, el LED sigue parpadeando.
  * Desbordamiento (OERR): se borra con clear_overrun(); el eco de un byte bloquea hasta ~1 ms (write espera TXIF), asi que
    una rafaga mas rapida que el eco puede perder bytes.
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.periph.uart import UART

CLOCK_HZ: Const[u32] = 4_000_000


def main() -> None:
    RD0.output()
    UART.start(baud=9600)
    pasadas: u16 = 0
    while True:
        if UART.overrun():
            UART.clear_overrun()
        if UART.rx_ready():
            UART.write(UART.recv())
        pasadas += 1
        if pasadas == 3000:
            pasadas = 0
            RD0.toggle()
