//! Registros de control de Intel HEX (#61): fin de archivo y direccion lineal extendida, validos e invalidos.

use pypic_pic14::{HexError, Image};

fn parse(txt: &str) -> Result<Image, (usize, String)> {
    Image::from_intel_hex(txt).map_err(|e| match e {
        HexError::Parse { line, reason } => (line, reason),
        other => panic!("{other:?}"),
    })
}

// ---- fin de archivo (tipo 01) ------------------------------------------------------------------------

#[test]
fn a_canonical_end_of_file_is_accepted() {
    assert_eq!(parse(":00000001FF\n").unwrap().len(), 0);
    assert_eq!(parse(":020000000528D1\n:00000001FF\n").unwrap().len(), 1);
}

#[test]
fn an_end_of_file_with_data_is_rejected_even_with_a_good_checksum() {
    // longitud 1, dato 00: checksum 01+01 = 02 -> FE
    let (line, why) = parse(":0100000100FE\n").unwrap_err();
    assert_eq!(line, 1);
    assert!(why.contains("registro 01") && why.contains("longitud"), "{why}");
}

#[test]
fn an_end_of_file_with_a_nonzero_address_is_rejected() {
    // :00 0001 01 -> checksum 00+00+01+01 = 02 -> FE
    let (line, why) = parse(":00000101FE\n").unwrap_err();
    assert_eq!(line, 1);
    assert!(why.contains("registro 01") && why.contains("direccion"), "{why}");
}

#[test]
fn the_line_number_points_at_the_bad_record() {
    let (line, why) = parse(":020000000528D1\n:0100000100FE\n").unwrap_err();
    assert_eq!(line, 2, "{why}");
}

// ---- direccion lineal extendida (tipo 04) -------------------------------------------------------------

#[test]
fn a_valid_extended_address_record_is_accepted() {
    // 04 con direccion 0000 y la base 0x0001 (el segmento de 64 KB siguiente): checksum 02+00+00+04+00+01 = 07 -> F9
    let img = parse(":020000040001F9\n:020000000528D1\n:00000001FF\n").unwrap();
    // la palabra cae en la direccion de bytes 0x10000, la de palabra 0x8000
    assert_eq!(img.get(0x8000), Some(0x2805));
    // y la forma con base 0000, la que escribe el exportador, tambien
    assert!(parse(":020000040000FA\n:00000001FF\n").is_ok());
}

#[test]
fn an_extended_address_with_a_nonzero_address_field_is_rejected() {
    // direccion 0002, dato 0000: 02+00+02+04 = 08 -> F8
    let (line, why) = parse(":020002040000F8\n:00000001FF\n").unwrap_err();
    assert_eq!(line, 1);
    assert!(why.contains("registro 04") && why.contains("direccion"), "{why}");
}

#[test]
fn an_extended_address_with_the_wrong_length_is_still_rejected() {
    // longitud 1: 01+00+00+04+00 = 05 -> FB
    let (_, why) = parse(":0100000400FB\n:00000001FF\n").unwrap_err();
    assert!(why.contains("registro 04") && why.contains("longitud"), "{why}");
}

#[test]
fn the_exporter_output_always_parses_back() {
    let mut img = Image::new();
    img.set(0x0000, 0x2805).unwrap();
    img.set(0x8001, 0x3FFF).unwrap(); // cruza a otro segmento de 64 KB
    img.set(0x2007, 0x20D4).unwrap();
    let hex = img.to_intel_hex(16);
    assert_eq!(Image::from_intel_hex(&hex).unwrap(), img);
}
