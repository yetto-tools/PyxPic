' Un bloque ASM es una rutina escrita en ensamblador PIC-AS, que se llama con GOSUB. Las variables se usan por su nombre (b0, w1, las de DIM) y la rutina
' termina en RETURN. Aqui SWAPF intercambia los dos nibbles de b0 en una sola instruccion y el resultado sale por la UART.
' Equivale a la idea de examples/python/asm-en-linea/inline_asm.py: el escape de rendimiento cuando el lenguaje no alcanza.
DEVICE PIC16F887
CLOCK INTERNAL 4 MHZ

b0 = $A5
GOSUB intercambiar        ' b0 = $5A
SERTXD(b0)
END

ASM intercambiar
    SWAPF b0,F            ; el cuerpo es ensamblador de pic-as: los comentarios van con punto y coma
    RETURN
END ASM
