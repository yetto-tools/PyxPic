//! Genera el .hex del blink de muestra: `cargo run -p pyxpic-pic14 --example blink_rd0 -- salida.hex`

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "blink_rd0.hex".into());
    let img = pyxpic_pic14::samples::blink_rd0();
    img.check_bounds(8192, (0x2000, 0x3FFF)).expect("dentro de la memoria del PIC16F887");
    std::fs::write(&path, img.to_intel_hex(16)).expect("escribir el .hex");
    println!("{path}: {} palabras", img.len());
}
