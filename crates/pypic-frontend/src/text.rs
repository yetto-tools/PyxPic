//! Texto a numero: `parse_dec(buf, n, error)` y `parse_hex(buf, n, error)` (docs/sdk-texto.md).
//!
//! Leen los primeros `n` bytes de un arreglo `uint8[N]` (por ejemplo la linea que dejo `UART.read_line`) y devuelven un `uint16`. El
//! error no se puede devolver junto al valor (el lenguaje no tiene tuplas): se escribe en una variable `bool` que se nombra, y el
//! valor es 0. La conversion la hace la IR con aritmetica de 16 bits y bifurcaciones: no hay rutina de runtime ni interprete.

/// La conversion de referencia: es lo que hacen el intérprete y el PIC, y lo que las pruebas comparan.
///
/// * `Dec`: solo cifras `0-9`, al menos una; el valor cabe en 16 bits.
/// * `Hex`: cifras `0-9`, `A-F` y `a-f`, al menos una, con un prefijo opcional `0x` o `0X`; el valor cabe en 16 bits (se admiten ceros a
///   la izquierda).
///
/// Cualquier otro byte, una entrada vacia o un desbordamiento es un error (`None`).
pub fn parse(op: TextOp, bytes: &[u8]) -> Option<u16> {
    let (radix, digits): (u32, &[u8]) = match op {
        TextOp::ParseDec => (10, bytes),
        TextOp::ParseHex => {
            let prefixed = bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] | 0x20) == b'x';
            (16, if prefixed { &bytes[2..] } else { bytes })
        }
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &c in digits {
        let d = (c as char).to_digit(radix)?;
        // to_digit acepta A-F y a-f en base 16 y nada mas que 0-9 en base 10: justo lo definido arriba
        v = v * radix + d;
        if v > 0xFFFF {
            return None;
        }
    }
    Some(v as u16)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOp {
    ParseDec,
    ParseHex,
}

/// Una llamada a `parse_dec` / `parse_hex` ya resuelta: la operacion y el arreglo (indice en `Program.arrays`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextCall {
    pub op: TextOp,
    pub arr: usize,
}
