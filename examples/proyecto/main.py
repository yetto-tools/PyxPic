"""Parpadeo de RD0 cada 500 ms. El dispositivo, el reloj y la configuracion no estan aqui: salen del perfil de pypic.toml.

Con `pypic build` (perfil 'interna') sale un .hex para el oscilador interno de 4 MHz; con `pypic build --target cristal`, el mismo
programa para un cristal de 20 MHz. Si este archivo declarara su propio dispositivo o reloj, mandaria el programa sobre el perfil.
"""
from pypic.periph.gpio import RD0
from pypic.system.time import pause


def main() -> None:
    RD0.output()
    while True:
        RD0.toggle()
        pause(500)
