"""Media movil de las ultimas 8 lecturas del ADC (AN0) con un arreglo circular en RAM; enciende RD0 si la media pasa de la mitad.

El arreglo muestras guarda los 8 bits altos de cada lectura (0..255). Es RAM fija: 8 bytes reservados al compilar, a cero al arrancar.
El indice pos se acota con & 7, asi nunca sale del arreglo (un indice fuera de rango se satura a la ultima posicion, ver
docs/sdk-arreglos.md). Las primeras 8 vueltas la media incluye muestras en cero del arranque.
"""
from pypic.device import PIC16F887
from pypic.periph.adc import ADC, AN0
from pypic.periph.gpio import RD0

CLOCK: Internal[4 * MHz]

muestras: uint8[8]
media: uint8 = 0


def main() -> None:
    global media
    RD0.output()
    ADC.start()
    pos: uint8 = 0
    while True:
        lectura: uint16 = ADC.read(AN0)
        muestras[pos] = uint8(lectura >> 2)
        pos = (pos + 1) & 7
        suma: uint16 = 0
        for i in range(8):
            suma += uint16(muestras[i])
        media = uint8(suma >> 3)
        if media > 127:
            RD0.on()
        else:
            RD0.off()
