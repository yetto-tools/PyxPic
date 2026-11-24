use pypic_pic14::{assemble, render_errors, samples, AsmError, Inst};

fn ok(src: &str) -> pypic_pic14::Assembled {
    assemble(src).unwrap_or_else(|e| panic!("{}", render_errors(&e, "t.asm")))
}

fn errs(src: &str) -> Vec<AsmError> {
    assemble(src).expect_err("esperaba errores")
}

fn code(src: &str) -> &'static str {
    let e = errs(src);
    assert_eq!(e.len(), 1, "{e:?}");
    e[0].code
}

fn msg(src: &str) -> String {
    errs(src).remove(0).message
}

// ---- prueba de oro: el ensamblador reproduce el programa verificado en Proteus ---------------------

#[test]
fn the_asm_blink_is_identical_to_the_hand_assembled_proteus_verified_program() {
    let a = ok(include_str!("../../../examples/asm/blink_rd0.asm"));
    assert_eq!(a.image, samples::blink_rd0());
    let hex = a.image.to_intel_hex(16);
    let on_disk = include_str!("../../../examples/hex/blink_rd0.hex").replace("\r\n", "\n");
    assert_eq!(hex, on_disk, "el .hex ensamblado debe ser el mismo archivo que se probo en Proteus");
    assert_eq!(a.symbols["delay"], 0x10);
    assert_eq!(a.symbols["loop"], 0x09);
    assert_eq!(a.symbols["RP0"], 5);
}

// ---- ida y vuelta con el desensamblador, sobre las 16 384 palabras ------------------------------------

#[test]
fn every_valid_instruction_survives_disassemble_then_assemble() {
    let mut n = 0;
    for w in 0u16..0x4000 {
        let Some(inst) = Inst::decode(w) else { continue };
        let text = inst.to_string();
        let a = assemble(&text).unwrap_or_else(|e| panic!("{text:?}: {}", render_errors(&e, "t")));
        assert_eq!(a.image.get(0), Some(inst.encode().unwrap()), "{text}");
        n += 1;
    }
    assert_eq!(n, 16008);
}

// ---- sintaxis ---------------------------------------------------------------------------------------

#[test]
fn comments_case_labels_and_expressions() {
    let a = ok("; comentario\n\n  movlw 0x0A ; minusculas\nLABEL: NOP\nlabel2:\n    goto label2\n    GOTO $\n    GOTO $+1\n    MOVLW 2+3\n    MOVLW 0b101\n    MOVLW 0x10-1\n");
    let w = |addr: u32| a.image.get(addr).unwrap();
    assert_eq!(w(0), Inst::Movlw(10).encode().unwrap());
    assert_eq!(a.symbols["LABEL"], 1); // los simbolos distinguen mayusculas
    assert_eq!(a.symbols["label2"], 2);
    assert_eq!(w(2), Inst::Goto(2).encode().unwrap()); // GOTO label2
    assert_eq!(w(3), Inst::Goto(3).encode().unwrap()); // GOTO $
    assert_eq!(w(4), Inst::Goto(5).encode().unwrap()); // GOTO $+1
    let (a5, a6, a7) = (w(5), w(6), w(7));
    assert_eq!((a5, a6, a7), (Inst::Movlw(5).encode().unwrap(), Inst::Movlw(5).encode().unwrap(), Inst::Movlw(15).encode().unwrap()));
}

#[test]
fn equ_constants_can_use_earlier_symbols_and_the_address() {
    let a = ok("BASE EQU 0x20\nNEXT EQU BASE+1\nHERE: NOP\nWHERE EQU $\n    MOVWF NEXT\n");
    assert_eq!((a.symbols["NEXT"], a.symbols["WHERE"]), (0x21, 1));
    assert_eq!(a.image.get(1), Some(Inst::Movwf(0x21).encode().unwrap()));
}

#[test]
fn org_and_dw_place_data_including_the_configuration_words() {
    let a = ok("    ORG 0x10\n    DW 1, 2, 0x3FFF\n    ORG 0x2007\n    DW 0x20D4\n");
    assert_eq!((a.image.get(0x10), a.image.get(0x11), a.image.get(0x12)), (Some(1), Some(2), Some(0x3FFF)));
    assert_eq!(a.image.get(0x2007), Some(0x20D4));
}

#[test]
fn listing_shows_address_word_and_source() {
    let a = ok("    MOVLW 0x2A ; k\nx:  NOP\n");
    assert_eq!(a.listing, "0000 302A      MOVLW 0x2A ; k\n0001 0000  x:  NOP\n");
}

// ---- errores con ubicacion y mensajes que guian ------------------------------------------------------

#[test]
fn unknown_and_foreign_instructions_explain_themselves() {
    assert_eq!(code("    FOO 1\n"), "ASM002");
    let e = errs("   FOO 1\n");
    assert_eq!((e[0].line, e[0].col), (1, 4));
    let m = msg("    MOVLB 1\n");
    assert!(m.contains("PIC16F1xxx") && m.contains("no existe en el PIC16 clasico"), "{m}");
    let m = msg("    SETC\n");
    assert!(m.contains("MPASM") && m.contains("BSF STATUS,C"), "{m}");
}

#[test]
fn byte_instructions_need_an_explicit_w_or_f_destination() {
    assert!(msg("    ADDWF 0x20\n").contains("falta el destino"));
    let m = msg("    ADDWF 0x20,1\n");
    assert!(m.contains("W o F") && m.contains("usa F"), "{m}");
    assert!(msg("    ADDWF 0x20,0\n").contains("usa W"));
    assert_eq!(code("    ADDWF 0x20,X\n"), "ASM003");
    ok("    ADDWF 0x20,w\n    ADDWF 0x20,F\n"); // W y F sin distinguir mayusculas
}

#[test]
fn operand_ranges_are_checked() {
    let m = msg("    MOVWF 0x188\n");
    assert!(m.contains("7 bits") && m.contains("RP0/RP1") && m.contains("banco 3"), "{m}");
    assert_eq!(code("    BSF 0x03,8\n"), "ASM004");
    assert_eq!(code("    MOVLW 256\n"), "ASM004");
    assert_eq!(code("    MOVLW -1\n"), "ASM004");
    assert_eq!(code("    DW 0x4000\n"), "ASM004");
}

#[test]
fn jumps_must_stay_in_page_zero() {
    let m = msg("    GOTO 0x800\n");
    assert!(m.contains("pagina 0") && m.contains("PCLATH"), "{m}");
    assert_eq!(code("    ORG 0x4000\n"), "ASM006");
}

#[test]
fn symbol_errors() {
    assert_eq!(code("    GOTO nowhere\n"), "ASM005");
    assert!(msg("    GOTO nowhere\n").contains("'nowhere'"));
    assert_eq!(code("a: NOP\na: NOP\n"), "ASM005");
    assert_eq!(code("nop: NOP\n"), "ASM005"); // palabra reservada
    assert_eq!(code("W EQU 1\n"), "ASM005");
    assert_eq!(code("X EQU LATER\nLATER EQU 1\n"), "ASM005"); // las constantes solo usan simbolos ya definidos
    assert_eq!(code("A EQU 1\nA EQU 2\n"), "ASM005");
}

#[test]
fn operand_count_and_syntax_errors() {
    assert_eq!(code("    NOP 1\n"), "ASM003");
    assert_eq!(code("    MOVLW\n"), "ASM003");
    assert_eq!(code("a: NOP\n    GOTO a,a\n"), "ASM003");
    assert_eq!(code("    MOVLW #5\n"), "ASM001");
    assert_eq!(code("    MOVLW 5 +\n"), "ASM003");
    assert_eq!(code("    MOVLW 12abc\n"), "ASM001");
}

#[test]
fn overlapping_addresses_are_rejected() {
    let e = errs("    ORG 0x10\n    NOP\n    ORG 0x10\n    NOP\n");
    assert_eq!((e.len(), e[0].code, e[0].line), (1, "ASM006", 4));
}

#[test]
fn all_errors_are_reported_in_source_order_with_the_rendered_format() {
    let e = errs("    FOO\n    MOVLW 999\n    GOTO nowhere\n");
    assert_eq!(e.iter().map(|x| (x.line, x.code)).collect::<Vec<_>>(), vec![(1, "ASM002"), (2, "ASM004"), (3, "ASM005")]);
    let out = render_errors(&e, "p.asm");
    assert!(out.starts_with("p.asm:1:5: error[ASM002]"), "{out}");
}
