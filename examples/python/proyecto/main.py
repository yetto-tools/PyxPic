"""Parpadeo de RD0 cada 500 ms. El dispositivo se declara aqui, en el codigo; el reloj y la configuracion salen del perfil de pyxpic.toml.

Con `pyxpic build` (perfil 'interna') sale un .hex para el oscilador interno de 4 MHz; con `pyxpic build --target cristal`, el mismo
programa para un cristal de 20 MHz. Si un perfil pidiera otro dispositivo, `pyxpic` lo rechaza: el codigo dice para que PIC esta escrito.
"""
from pyxpic.device import PIC16F887
from pyxpic.periph.gpio import RD0
from pyxpic.system.time import pause


def main() -> None:
    RD0.output()
    while True:
        RD0.toggle()
        pause(500)
