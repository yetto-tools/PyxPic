//! Macros del ensamblador PIC-AS: MACRO/ENDM, LOCAL, REPT, IRP, IRPC y RADIX (DS50002974F, 4.9).

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_pic14::{assemble_with, render_errors, AsmEnv, AsmError, Assembled, Inst};

fn env() -> AsmEnv {
    AsmEnv::from_pack(&DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap())
}

fn abs(body: &str) -> String {
    format!("#include <xc.inc>\nPSECT t,class=CODE,delta=2,abs\n{body}\n")
}

fn ok(body: &str) -> Assembled {
    let src = abs(body);
    assemble_with(&src, &env()).unwrap_or_else(|e| panic!("{}\n{src}", render_errors(&e, "t.s")))
}

fn errs(body: &str) -> Vec<AsmError> {
    assemble_with(&abs(body), &env()).expect_err("esperaba errores")
}

fn w(a: &Assembled, addr: u32) -> u16 {
    a.image.get(addr).unwrap_or_else(|| panic!("no hay palabra en {addr:#x}"))
}

fn enc(i: Inst) -> u16 {
    i.encode().unwrap()
}

#[test]
fn a_macro_substitutes_its_arguments() {
    let a = ok("movlf MACRO value,reg\n    movlw value\n    movwf reg\nENDM\n    movlf 2,0x20\n    movlf 7,0x21");
    assert_eq!((w(&a, 0), w(&a, 1)), (enc(Inst::Movlw(2)), enc(Inst::Movwf(0x20))));
    assert_eq!((w(&a, 2), w(&a, 3)), (enc(Inst::Movlw(7)), enc(Inst::Movwf(0x21))));
}

#[test]
fn ampersand_concatenates_an_argument_with_other_text() {
    // el ejemplo de la guia: PORT&port con port = B es PORTB
    let a = ok("loadPort MACRO port,value\n    movlw value\n    movwf PORT&port\nENDM\n    loadPort B,0x55\n    loadPort D,1");
    assert_eq!((w(&a, 1), w(&a, 3)), (enc(Inst::Movwf(0x06)), enc(Inst::Movwf(0x08))));
}

#[test]
fn angle_brackets_quote_an_argument_with_commas() {
    let a = ok("ld MACRO ops\n    movf ops\nENDM\n    ld <0x20,w>\n    ld <0x21,f>");
    use pyxpic_pic14::Dest::{F, W};
    assert_eq!((w(&a, 0), w(&a, 1)), (enc(Inst::Movf(0x20, W)), enc(Inst::Movf(0x21, F))));
}

#[test]
fn local_gives_every_expansion_its_own_labels() {
    let a = ok("down MACRO count\n    LOCAL more\nmore:\n    decfsz count,f\n    goto more\nENDM\n    down 0x20\n    down 0x21");
    // dos expansiones sin etiquetas repetidas; cada goto vuelve a su decfsz
    assert_eq!((w(&a, 1), w(&a, 3)), (enc(Inst::Goto(0)), enc(Inst::Goto(2))));
    assert!(a.symbols.keys().any(|k| k.starts_with("??0001")) && a.symbols.keys().any(|k| k.starts_with("??0002")), "{:?}", a.symbols.keys().collect::<Vec<_>>());
}

#[test]
fn rept_repeats_a_block() {
    let a = ok("REPT 3\n    addwf 0x20,w\nENDM\n    nop");
    for i in 0..3 {
        assert_eq!(w(&a, i), enc(Inst::Addwf(0x20, pyxpic_pic14::Dest::W)));
    }
    assert_eq!(w(&a, 3), enc(Inst::Nop));
    assert!(a.image.get(4).is_none());
}

#[test]
fn irp_and_irpc_iterate_over_a_list_and_over_characters() {
    let a = ok("IRP number,1Ah,2Bh,3Ch\n    DW number\nENDM\nIRPC char,ABC\n    DB 'char'\nENDM");
    let got: Vec<u16> = (0..6).map(|i| w(&a, i)).collect();
    assert_eq!(got, vec![0x1A, 0x2B, 0x3C, b'A' as u16, b'B' as u16, b'C' as u16]);
}

#[test]
fn macros_nest_and_a_label_may_precede_the_invocation() {
    let a = ok("two MACRO r\n    incf r,f\n    incf r,f\nENDM\nfour MACRO r\n    two r\n    two r\nENDM\nstart: four 0x20\nend_label: nop");
    assert_eq!(a.symbols["start"], 0);
    assert_eq!(a.symbols["end_label"], 4);
    assert!((0..4).all(|i| w(&a, i) == enc(Inst::Incf(0x20, pyxpic_pic14::Dest::F))));
    // un REPT dentro de una macro y una macro dentro de un REPT
    let a = ok("blk MACRO\n    REPT 2\n        nop\n    ENDM\nENDM\nREPT 2\n    blk\nENDM");
    assert_eq!(a.image.len(), 4);
}

#[test]
fn missing_arguments_are_empty() {
    // `b` es una linea entera: vacia si no se pasa, y una instruccion si se pasa
    let a = ok("m MACRO a,b
    movlw a
    b
ENDM
    m 3
    m 4,nop");
    assert_eq!((w(&a, 0), w(&a, 1), w(&a, 2)), (enc(Inst::Movlw(3)), enc(Inst::Movlw(4)), enc(Inst::Nop)));
    assert!(a.image.get(3).is_none());
}

#[test]
fn macro_errors() {
    let m = |body: &str| errs(body).remove(0).message;
    assert!(m("m MACRO\n nop\n").contains("falta el ENDM"));
    assert!(m("ENDM\n").contains("sin un MACRO"));
    assert!(m("m MACRO a\n nop\nENDM\n m 1,2\n").contains("1 argumento(s)"));
    assert!(m("m MACRO\n m\nENDM\n m\n").contains("sin fin"));
    assert!(m("m MACRO\n nop\nENDM\nm MACRO\n nop\nENDM\n").contains("ya esta definida"));
    assert!(m("movlw MACRO\n nop\nENDM\n").contains("reservada"));
    assert!(m("REPT x\n nop\nENDM\n").contains("numero constante"));
    assert!(m("REPT 5000\n nop\nENDM\n").contains("4096"));
    assert!(m("IRP a\n nop\nENDM\n").contains("IRP necesita"));
    assert!(m("LOCAL x\n").contains("dentro de un MACRO"));
    assert!(m("EXITM\n").contains("EXITM"));
}

#[test]
fn errors_inside_an_expansion_point_at_the_invocation_line() {
    let e = errs("bad MACRO\n    movlw 999\nENDM\n    nop\n    bad");
    // linea 1 #include, 2 PSECT, 3 MACRO, 4 cuerpo, 5 ENDM, 6 nop, 7 bad
    assert_eq!((e[0].line, e[0].code), (7, "ASM004"), "{e:?}");
}

#[test]
fn the_listing_shows_the_expanded_words_under_the_invocation() {
    let a = ok("two MACRO\n    nop\n    nop\nENDM\n    two");
    assert!(a.listing.contains("0000 0000      two") && a.listing.contains("0001 0000\n"), "{}", a.listing);
}

#[test]
fn radix_changes_the_base_of_numbers_without_a_suffix() {
    let a = ok("RADIX hex\n    movlw 10\n    movlw 0Ah\nRADIX oct\n    movlw 17\nRADIX dec\n    movlw 10");
    let got: Vec<u16> = (0..4).map(|i| w(&a, i)).collect();
    assert_eq!(got, vec![enc(Inst::Movlw(0x10)), enc(Inst::Movlw(10)), enc(Inst::Movlw(15)), enc(Inst::Movlw(10))]);
    assert_eq!(errs("RADIX bin\n")[0].code, "ASM003");
}

#[test]
fn directives_that_the_guide_does_not_define_for_14_bit_words_are_rejected_with_a_reason() {
    for d in ["DDW 1", "DQW 1", "DABS 1,0x20,2"] {
        let m = errs(d).remove(0).message;
        assert!(m.contains("no esta implementada") && m.contains("14 bits"), "{d}: {m}");
    }
}
