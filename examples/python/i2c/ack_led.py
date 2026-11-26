"""Diagnostico del bus I2C con un LED en RD0: direcciona la mochila 0x27 (0x4E con R/W en 0) y muestra si respondio ACK.

Que se ve:
  * un destello de 300 ms al arrancar: el programa corre (si se repite una y otra vez, el PIC se esta reiniciando);
  * despues, LED fijo encendido: la mochila respondio ACK (el bus funciona);
  * despues, LED parpadeando rapido: nadie respondio (direccion, cableado o pull-ups);
  * despues, LED apagado y quieto: el bus no avanza (SDA o SCL a masa, sin pull-ups o unidos entre si).
Conexion: RC3/SCL y RC4/SDA con pull-ups de 4,7 kohm a VDD; LED con resistencia en RD0. Entrada: ninguna.
"""
from pypic.device import PIC16F887
from pypic.periph.gpio import RD0
from pypic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT, TRISC
from pypic.system.time import pause

CLOCK: Internal[4 * MHz]

DIRECCION: uint8 = 0x4E        # 0x27 << 1


def reposo() -> None:
    while (SSPCON2.read() & 0x1F) != 0:
        pass


@init_device
def configurar() -> None:
    RD0.output()


def main() -> None:
    RD0.on()
    pause(300)
    RD0.off()
    TRISC.write(TRISC.read() | 0x18)
    SSPADD.write(9)                       # 100 kHz a 4 MHz
    SSPCON.write(0x28)                    # I2C maestro
    SSPCON2.SEN.set()                     # START
    reposo()
    SSPBUF.write(DIRECCION)
    while SSPSTAT.R_W.value() != 0:
        pass
    nack: uint8 = SSPCON2.ACKSTAT.value() # 0 = hubo ACK
    SSPCON2.PEN.set()                     # STOP
    reposo()
    if nack == 0:
        RD0.on()
        while True:
            pass
    while True:
        RD0.toggle()
        pause(100)
