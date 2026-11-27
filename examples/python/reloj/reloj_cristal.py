"""Parpadeo de RD0 con un cristal de 20 MHz en OSC1/OSC2: 500 ms encendido y 500 ms apagado, exactos al ciclo de instruccion.

El reloj se declara en dos lineas, sin imports: CLOCK: Crystal y FREQUENCY: 20MHz eligen el modo HS de la palabra de configuracion (FOSC) y fijan la
frecuencia de las esperas. Otras formas:

    CLOCK: Internal / FREQUENCY: 8MHz       oscilador interno: 31 kHz a 8 MHz (el compilador escribe OSCCON y espera a que sea estable)
    CLOCK: Internal                         sin FREQUENCY: 4 MHz, el valor de reset
    CLOCK: Crystal / FREQUENCY: 4MHz        cristal: LP 32,768 kHz, XT 0,1 a 4 MHz, HS 1 a 20 MHz (el compilador elige el modo)
    CLOCK: Crystal / FREQUENCY: 11.0592MHz  cualquier valor dentro del rango del modo; OSC_MODE: HS fuerza el modo

Con cristal, RA6 (OSC2) y RA7 (OSC1) son del oscilador: usarlos como E/S es el error PYP2150. El circuito (el cristal y sus dos
capacitores a tierra) es del montaje: el compilador no puede comprobarlo. NO probado en hardware, solo en simulador.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.system.time import pause

CLOCK: Crystal
FREQUENCY: 20MHz


def main() -> None:
    RD0.output()
    while True:
        RD0.on()
        pause(2000)
        RD0.off()
        pause(2000)
