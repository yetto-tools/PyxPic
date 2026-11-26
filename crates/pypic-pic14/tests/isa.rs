//! Codificacion PIC14 verificada contra DS40001291H, tabla 15-2 (p.227).

use std::collections::BTreeSet;

use pypic_pic14::{disassemble, Cycles, Dest, EncodeError, Flag, Inst};
use Dest::{F, W};
use Inst::*;

/// (instruccion, palabra de 14 bits) calculados a mano desde la tabla del datasheet.
fn vectors() -> Vec<(Inst, u16)> {
    vec![
        // byte-oriented: 00 oooo dfff ffff
        (Addwf(0x20, F), 0x07A0),
        (Addwf(0x20, W), 0x0720),
        (Andwf(0x7F, W), 0x057F),
        (Clrf(0x20), 0x01A0),
        (Clrw, 0x0100),
        (Comf(0x05, F), 0x0985),
        (Decf(0x05, W), 0x0305),
        (Decfsz(0x05, F), 0x0B85),
        (Incf(0x05, F), 0x0A85),
        (Incfsz(0x05, W), 0x0F05),
        (Iorwf(0x05, W), 0x0405),
        (Movf(0x05, F), 0x0885),
        (Movwf(0x05), 0x0085),
        (Nop, 0x0000),
        (Rlf(0x05, F), 0x0D85),
        (Rrf(0x05, W), 0x0C05),
        (Subwf(0x05, F), 0x0285),
        (Swapf(0x05, W), 0x0E05),
        (Xorwf(0x05, F), 0x0685),
        // bit-oriented: 01 oobb bfff ffff
        (Bcf(0x03, 5), 0x1283), // BCF STATUS,RP0
        (Bsf(0x03, 5), 0x1683), // BSF STATUS,RP0
        (Bsf(0x03, 6), 0x1703), // BSF STATUS,RP1
        (Btfsc(0x03, 2), 0x1903),
        (Btfss(0x03, 0), 0x1C03),
        (Bcf(0x7F, 7), 0x13FF),
        // literal and control
        (Addlw(0xFF), 0x3EFF),
        (Andlw(0x0F), 0x390F),
        (Call(0x123), 0x2123),
        (Clrwdt, 0x0064),
        (Goto(0x7FF), 0x2FFF),
        (Goto(0x005), 0x2805),
        (Iorlw(0x80), 0x3880),
        (Movlw(0x2A), 0x302A),
        (Retfie, 0x0009),
        (Retlw(0x55), 0x3455),
        (Return, 0x0008),
        (Sleep, 0x0063),
        (Sublw(0x01), 0x3C01),
        (Xorlw(0xFF), 0x3AFF),
    ]
}

#[test]
fn encode_matches_datasheet_vectors() {
    for (inst, word) in vectors() {
        assert_eq!(inst.encode(), Ok(word), "{inst} deberia codificar a {word:#06x}");
    }
}

#[test]
fn decode_matches_datasheet_vectors() {
    for (inst, word) in vectors() {
        assert_eq!(Inst::decode(word), Some(inst), "{word:#06x} deberia decodificar a {inst}");
    }
}

#[test]
fn vectors_cover_all_35_instructions() {
    let names: BTreeSet<&str> = vectors().iter().map(|(i, _)| i.mnemonic()).collect();
    assert_eq!(names.len(), 35, "{names:?}");
}

/// Bits sin importancia ("x") de la tabla, por instruccion.
fn dont_care_mask(i: &Inst) -> u16 {
    match i {
        Nop => 0x0060,
        Clrw => 0x007F,
        Movlw(_) | Retlw(_) => 0x0300,
        Sublw(_) | Addlw(_) => 0x0100,
        _ => 0,
    }
}

#[test]
fn exhaustive_over_every_14_bit_word() {
    let mut valid = 0u32;
    let mut mnemonics = BTreeSet::new();
    for w in 0u16..0x4000 {
        let Some(i) = Inst::decode(w) else { continue };
        valid += 1;
        mnemonics.insert(i.mnemonic());
        let e = i.encode().unwrap();
        assert_eq!(Inst::decode(e), Some(i), "ida y vuelta de {w:#06x}");
        // la palabra canonica solo difiere de `w` en los bits "x" de la tabla
        let m = dont_care_mask(&i);
        assert_eq!(e & !m, w & !m, "{w:#06x} -> {i} -> {e:#06x}");
        assert_eq!(e & m, 0, "forma canonica con x = 0");
    }
    assert_eq!(mnemonics.len(), 35);
    // 14 ops con d x 256 + CLRF/MOVWF/CLRW x 128 + NOP 4 + 4 sin operandos + 4 de bits x 1024
    // + CALL/GOTO x 2048 + literales (MOVLW/RETLW 1024, IORLW/ANDLW/XORLW 256, SUBLW/ADDLW 512)
    assert_eq!(valid, 14 * 256 + 3 * 128 + 4 + 4 + 4 * 1024 + 2 * 2048 + 2 * 1024 + 3 * 256 + 2 * 512);
}

#[test]
fn invalid_words_are_rejected() {
    assert_eq!(Inst::decode(0x4000), None); // fuera de 14 bits
    assert_eq!(Inst::decode(0xFFFF), None);
    for w in 0x3B00..=0x3BFF {
        assert_eq!(Inst::decode(w), None, "{w:#06x}");
    }
    for w in [0x0001, 0x0007, 0x000A, 0x001F, 0x0062, 0x0065, 0x007F] {
        assert_eq!(Inst::decode(w), None, "{w:#06x}");
    }
}

#[test]
fn dont_care_variants_decode_to_the_same_instruction() {
    for w in [0x0000, 0x0020, 0x0040, 0x0060] {
        assert_eq!(Inst::decode(w), Some(Nop));
    }
    assert_eq!(Inst::decode(0x017F), Some(Clrw));
    assert_eq!(Inst::decode(0x3355), Some(Movlw(0x55)));
    assert_eq!(Inst::decode(0x3755), Some(Retlw(0x55)));
    assert_eq!(Inst::decode(0x3D01), Some(Sublw(0x01)));
    assert_eq!(Inst::decode(0x3F01), Some(Addlw(0x01)));
}

#[test]
fn operand_ranges_are_checked() {
    assert_eq!(Movwf(0x80).encode(), Err(EncodeError::FileOutOfRange(0x80)));
    assert_eq!(Addwf(200, W).encode(), Err(EncodeError::FileOutOfRange(200)));
    assert_eq!(Bsf(0x03, 8).encode(), Err(EncodeError::BitOutOfRange(8)));
    assert_eq!(Btfsc(0x80, 0).encode(), Err(EncodeError::FileOutOfRange(0x80)));
    assert_eq!(Goto(0x800).encode(), Err(EncodeError::AddressOutOfRange(0x800)));
    assert_eq!(Call(0xFFFF).encode(), Err(EncodeError::AddressOutOfRange(0xFFFF)));
    assert!(Movlw(0xFF).encode().is_ok() && Goto(0x7FF).encode().is_ok() && Bsf(0x7F, 7).encode().is_ok());
}

#[test]
fn display_format() {
    assert_eq!(Addwf(0x20, W).to_string(), "ADDWF 0x20,W");
    assert_eq!(Movwf(0x05).to_string(), "MOVWF 0x05");
    assert_eq!(Bsf(0x03, 5).to_string(), "BSF 0x03,5");
    assert_eq!(Movlw(0x2A).to_string(), "MOVLW 0x2a");
    assert_eq!(Goto(0x005).to_string(), "GOTO 0x0005");
    assert_eq!(Return.to_string(), "RETURN");
}

#[test]
fn cycles_follow_the_table() {
    for i in [Decfsz(1, W), Incfsz(1, W), Btfsc(1, 0), Btfss(1, 0)] {
        assert_eq!(i.cycles(), Cycles::OneOrTwo, "{i}");
    }
    for i in [Call(0), Goto(0), Return, Retfie, Retlw(0)] {
        assert_eq!(i.cycles(), Cycles::Two, "{i}");
    }
    for i in [Addwf(1, W), Movlw(1), Nop, Sleep, Clrwdt, Bsf(1, 0), Swapf(1, F)] {
        assert_eq!(i.cycles(), Cycles::One, "{i}");
    }
}

#[test]
fn status_flags_follow_the_table() {
    assert_eq!(Addwf(1, W).status_affected(), &[Flag::C, Flag::DC, Flag::Z]);
    assert_eq!(Subwf(1, W).status_affected(), &[Flag::C, Flag::DC, Flag::Z]);
    assert_eq!(Addlw(1).status_affected(), &[Flag::C, Flag::DC, Flag::Z]);
    assert_eq!(Sublw(1).status_affected(), &[Flag::C, Flag::DC, Flag::Z]);
    assert_eq!(Rlf(1, F).status_affected(), &[Flag::C]);
    assert_eq!(Rrf(1, F).status_affected(), &[Flag::C]);
    assert_eq!(Andwf(1, W).status_affected(), &[Flag::Z]);
    assert_eq!(Clrw.status_affected(), &[Flag::Z]);
    assert_eq!(Sleep.status_affected(), &[Flag::TO, Flag::PD]);
    assert_eq!(Clrwdt.status_affected(), &[Flag::TO, Flag::PD]);
    // el datasheet recomienda estas para tocar STATUS: no afectan a ningun flag
    for i in [Bcf(3, 5), Bsf(3, 5), Swapf(3, W), Movwf(3), Movlw(1), Goto(0), Call(0), Nop] {
        assert!(i.status_affected().is_empty(), "{i}");
    }
}

#[test]
fn disassembler_lists_addresses_words_and_invalid_data() {
    let out = disassemble(&[0x2805, 0x1683, 0x3B00, 0x0008], 0);
    assert_eq!(out, "0000: 2805  GOTO 0x0005\n0001: 1683  BSF 0x03,5\n0002: 3B00  DW 0x3b00\n0003: 0008  RETURN\n");
    assert!(disassemble(&[0x0000], 0x10).starts_with("0010: 0000  NOP"));
}
