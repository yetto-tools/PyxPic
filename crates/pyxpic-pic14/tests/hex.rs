use pyxpic_pic14::{HexError, Image};

fn img(pairs: &[(u32, u16)]) -> Image {
    let mut i = Image::new();
    for &(a, w) in pairs {
        i.set(a, w).unwrap();
    }
    i
}

// ---- vectores calculados a mano (suma de bytes del registro + checksum = 0 mod 256) -------

#[test]
fn single_word_goto_matches_hand_computed_record() {
    // GOTO 0x0005 = 0x2805 -> bytes 05 28; 02+00+00+00+05+28 = 2F -> checksum D1
    assert_eq!(img(&[(0, 0x2805)]).to_intel_hex(16), ":020000000528D1\n:00000001FF\n");
}

#[test]
fn two_words_in_one_record() {
    // 05 28 83 16: 04+05+28+83+16 = CA -> checksum 36
    assert_eq!(img(&[(0, 0x2805), (1, 0x1683)]).to_intel_hex(16), ":040000000528831636\n:00000001FF\n");
}

#[test]
fn config_word_uses_doubled_byte_address() {
    // palabra 0x2007 -> byte 0x400E; 0x3FF1 -> bytes F1 3F; 02+40+0E+00+F1+3F = 180 -> checksum 80
    assert_eq!(img(&[(0x2007, 0x3FF1)]).to_intel_hex(16), ":02400E00F13F80\n:00000001FF\n");
}

#[test]
fn empty_image_is_just_the_eof_record() {
    assert_eq!(Image::new().to_intel_hex(16), ":00000001FF\n");
}

// ---- estructura de los registros ----------------------------------------------------------

#[test]
fn long_runs_split_by_max_bytes() {
    let words: Vec<(u32, u16)> = (0..9).map(|i| (i, 0x3000 + i as u16)).collect();
    let hex = img(&words).to_intel_hex(16);
    let lines: Vec<&str> = hex.lines().collect();
    assert_eq!(lines.len(), 3); // 16 bytes + 2 bytes + EOF
    assert!(lines[0].starts_with(":10000000"), "{}", lines[0]);
    assert!(lines[1].starts_with(":02001000"), "{}", lines[1]); // continua en el byte 0x10
}

#[test]
fn gaps_start_new_records() {
    let hex = img(&[(0, 0x0000), (0x10, 0x0008)]).to_intel_hex(16);
    let lines: Vec<&str> = hex.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[1].starts_with(":02002000"), "{}", lines[1]); // palabra 0x10 -> byte 0x20
}

#[test]
fn records_never_cross_a_64k_boundary_and_use_extended_address() {
    // palabras 0x7FFE..0x8001 -> bytes 0xFFFC..0x10003
    let hex = img(&[(0x7FFE, 0x3001), (0x7FFF, 0x3002), (0x8000, 0x3003), (0x8001, 0x3004)]).to_intel_hex(16);
    let lines: Vec<&str> = hex.lines().collect();
    assert!(lines[0].starts_with(":04FFFC00"), "{}", lines[0]);
    assert_eq!(lines[1], ":020000040001F9"); // 02+04+00+01 = 07 -> F9
    assert!(lines[2].starts_with(":04000000"), "{}", lines[2]);
    assert_eq!(lines[3], ":00000001FF");
}

#[test]
fn small_max_bytes_is_clamped_to_even_and_at_least_two() {
    let hex = img(&[(0, 0x3001), (1, 0x3002)]).to_intel_hex(1);
    assert_eq!(hex.lines().count(), 3); // un registro por palabra + EOF
}

// ---- lectura ------------------------------------------------------------------------------

#[test]
fn roundtrip_with_gaps_config_and_extended_addresses() {
    let mut i = Image::new();
    i.extend(0, &[0x2805, 0x0000, 0x0000, 0x0000]).unwrap();
    i.extend(4, &(0..40).map(|k| 0x3000 + k).collect::<Vec<u16>>()).unwrap();
    i.set(0x1FFF, 0x3FFF).unwrap();
    i.set(0x2007, 0x3FF1).unwrap();
    i.set(0x2008, 0x3EFF).unwrap();
    i.extend(0x7FFE, &[0x1111, 0x2222, 0x3333]).unwrap(); // cruza 64 KiB
    for max in [2, 6, 16, 32, 255] {
        assert_eq!(Image::from_intel_hex(&i.to_intel_hex(max)).unwrap(), i, "max_bytes = {max}");
    }
}

#[test]
fn accepts_crlf_blank_lines_and_lowercase() {
    let txt = ":020000000528d1\r\n\r\n:00000001ff\r\n";
    assert_eq!(Image::from_intel_hex(txt).unwrap(), img(&[(0, 0x2805)]));
}

fn parse_err(txt: &str) -> String {
    match Image::from_intel_hex(txt).unwrap_err() {
        HexError::Parse { reason, .. } => reason,
        e => panic!("{e:?}"),
    }
}

#[test]
fn parser_rejects_malformed_input() {
    assert!(parse_err("020000000528D1\n:00000001FF\n").contains("':'"));
    assert!(parse_err(":020000000528D2\n:00000001FF\n").contains("checksum")); // checksum +1
    assert!(parse_err(":02000000052\n:00000001FF\n").contains("longitud")); // numero impar de digitos
    assert!(parse_err(":0200000005XXD1\n:00000001FF\n").contains("caracteres"));
    assert!(parse_err(":030000000528D1\n:00000001FF\n").contains("no coincide")); // len declarada = 3
    assert!(parse_err(":020000000528D1\n").contains("fin de archivo")); // sin EOF
    assert!(parse_err(":00000001FF\n:020000000528D1\n").contains("despues"));
    assert!(parse_err(":0200000205A057\n:00000001FF\n").contains("no soportado")); // tipo 02; 02+02+05+A0 = A9 -> 57
    assert!(parse_err(":01000000FF00\n:00000001FF\n").contains("alinear")); // 1 byte; 01+FF = 100 -> 00
    assert!(parse_err(":020001000528D0\n:00000001FF\n").contains("alinear")); // direccion impar
}

#[test]
fn parser_rejects_words_wider_than_14_bits() {
    // bytes FF FF = 0xFFFF; 02+FF+FF = 200 -> checksum 00
    assert!(parse_err(":02000000FFFF00\n:00000001FF\n").contains("14 bits"));
}

// ---- imagen -------------------------------------------------------------------------------

#[test]
fn image_rejects_wide_words_and_conflicts() {
    let mut i = Image::new();
    assert_eq!(i.set(0, 0x4000), Err(HexError::WordTooWide { addr: 0, word: 0x4000 }));
    i.set(5, 0x3000).unwrap();
    assert!(i.set(5, 0x3000).is_ok()); // misma palabra: idempotente
    assert_eq!(i.set(5, 0x3001), Err(HexError::Conflict { addr: 5 }));
    assert_eq!(i.len(), 1);
    assert_eq!(i.get(5), Some(0x3000));
    assert_eq!(i.get(6), None);
}

#[test]
fn bounds_check_uses_program_size_and_config_zone() {
    // PIC16F887: 8192 palabras de programa y configuracion 0x2000..=0x3FFF
    let ok = img(&[(0, 0), (0x1FFF, 0), (0x2007, 0x3FFF), (0x2008, 0x3FFF)]);
    assert!(ok.check_bounds(8192, (0x2000, 0x3FFF)).is_ok());
    let bad = img(&[(0x2000 - 1 + 1, 0)]); // 0x2000 cae en la zona de configuracion
    assert!(bad.check_bounds(8192, (0x2007, 0x2008)).is_err());
    assert_eq!(img(&[(0x4000, 0)]).check_bounds(8192, (0x2000, 0x3FFF)), Err(HexError::OutOfBounds { addr: 0x4000 }));
}
