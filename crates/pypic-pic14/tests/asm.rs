//! Ensamblador con la sintaxis de MPLAB XC8 PIC Assembler (pic-as, DS50002974F).

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_pic14::{assemble, assemble_with, render_errors, samples, AsmEnv, AsmError, Inst};

fn env() -> AsmEnv {
    AsmEnv::from_pack(&DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap())
}

fn env_with(opts: &[&str]) -> AsmEnv {
    let mut e = env();
    e.add_link_options(&opts.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
    e
}

fn ok_with(src: &str, e: &AsmEnv) -> pypic_pic14::Assembled {
    assemble_with(src, e).unwrap_or_else(|er| panic!("{}\n{src}", render_errors(&er, "t.s")))
}

fn ok(src: &str) -> pypic_pic14::Assembled {
    ok_with(src, &env())
}

fn errs(src: &str) -> Vec<AsmError> {
    assemble_with(src, &env()).expect_err("esperaba errores")
}

fn code(src: &str) -> &'static str {
    let e = errs(src);
    assert_eq!(e.len(), 1, "{e:?}");
    e[0].code
}

fn msg(src: &str) -> String {
    errs(src).remove(0).message
}

/// Programa de prueba: un psect absoluto de codigo (empieza en 0) con `body`.
fn abs(body: &str) -> String {
    format!("PSECT t,class=CODE,delta=2,abs\n{body}\n")
}

fn w(a: &pypic_pic14::Assembled, addr: u32) -> u16 {
    a.image.get(addr).unwrap_or_else(|| panic!("no hay palabra en {addr:#x}"))
}

fn enc(i: Inst) -> u16 {
    i.encode().unwrap()
}

// ---- prueba de oro: el ensamblador reproduce el programa verificado en Proteus ---------------------

#[test]
fn the_pic_as_blink_is_identical_to_the_hand_assembled_proteus_verified_program() {
    let e = env_with(&["-presetVec=0h", "-pintentry=4h"]);
    let a = ok_with(include_str!("../../../examples/asm/blink_rd0.s"), &e);
    assert_eq!(a.image, samples::blink_rd0());
    let hex = a.image.to_intel_hex(16);
    let on_disk = include_str!("../../../examples/hex/blink_rd0.hex").replace("\r\n", "\n");
    assert_eq!(hex, on_disk, "el .hex ensamblado debe ser el mismo archivo que se probo en Proteus");
    assert_eq!(a.symbols["delay"], 0x10);
    assert_eq!(a.symbols["loop"], 0x09);
    assert_eq!((a.symbols["C1"], a.symbols["C2"], a.symbols["C3"]), (0x20, 0x21, 0x22));
}

// ---- ida y vuelta con el desensamblador, sobre las 16 384 palabras ------------------------------------

#[test]
fn every_valid_instruction_survives_disassemble_then_assemble() {
    let mut n = 0;
    for word in 0u16..0x4000 {
        let Some(inst) = Inst::decode(word) else { continue };
        let text = abs(&inst.to_string());
        let a = assemble(&text).unwrap_or_else(|e| panic!("{text:?}: {}", render_errors(&e, "t")));
        assert_eq!(a.image.get(0), Some(inst.encode().unwrap()), "{text}");
        n += 1;
    }
    assert_eq!(n, 16008);
}

// ---- numeros y expresiones ---------------------------------------------------------------------------

#[test]
fn numbers_use_the_pic_as_radix_suffixes() {
    let a = assemble(&abs("movlw 0x1F\nmovlw 1Fh\nmovlw 0FFh\nmovlw 101B\nmovlw 17o\nmovlw 17q\nmovlw 10d\nmovlw 10\nmovlw 'A'")).unwrap();
    let got: Vec<u16> = (0..9).map(|i| w(&a, i)).collect();
    let want: Vec<u16> = [0x1F, 0x1F, 0xFF, 5, 15, 15, 10, 10, 65].iter().map(|&k| enc(Inst::Movlw(k))).collect();
    assert_eq!(got, want);
}

#[test]
fn a_gpasm_binary_literal_gets_a_migration_hint() {
    let e = assemble_with(&abs("movlw 0b101"), &env()).unwrap_err();
    assert_eq!(e[0].code, "ASM001");
    assert!(e[0].message.contains("101B"), "{}", e[0].message);
}

#[test]
fn expressions_are_evaluated_left_to_right_like_pic_as() {
    // la guia: 5 + 1 * 2 vale 12; con parentesis, 5 + (1 * 2) vale 7
    let a = assemble(&abs("movlw 5 + 1 * 2\nmovlw 5 + (1 * 2)\nmovlw 77 mod 4\nmovlw low(0x1234)\nmovlw high(0x1234)\nmovlw not 0F0h and 0FFh\nmovlw 1 shl 4\nmovlw 0F0h shr 4")).unwrap();
    let want = [12u8, 7, 1, 0x34, 0x12, 0x0F, 16, 0x0F];
    for (i, k) in want.iter().enumerate() {
        assert_eq!(w(&a, i as u32), enc(Inst::Movlw(*k)), "movlw #{i}");
    }
}

#[test]
fn the_location_counter_and_labels_work_in_expressions() {
    let a = ok(&abs("start:\n    goto start\n    goto $\n    goto $+2\n    nop\n    goto start+1"));
    assert_eq!(w(&a, 0), enc(Inst::Goto(0)));
    assert_eq!(w(&a, 1), enc(Inst::Goto(1)));
    assert_eq!(w(&a, 2), enc(Inst::Goto(4)));
    assert_eq!(w(&a, 4), enc(Inst::Goto(1)));
}

#[test]
fn comments_may_be_semicolon_slash_or_block_and_case_rules_follow_pic_as() {
    let a = ok(&abs("; comentario\n  MoVlW 0x0A ; minusculas y mayusculas\n  /* bloque\n     varias lineas */ nop // otro\nLabel: nop\nlabel: nop"));
    assert_eq!(w(&a, 0), enc(Inst::Movlw(10)));
    assert_eq!(w(&a, 1), enc(Inst::Nop));
    // los simbolos distinguen mayusculas, los mnemonicos no
    assert_eq!((a.symbols["Label"], a.symbols["label"]), (2, 3));
}

// ---- destinos ----------------------------------------------------------------------------------------

#[test]
fn destination_accepts_w_f_0_1_and_defaults_to_f() {
    let a = assemble(&abs("addwf 0x20,w\naddwf 0x20,W\naddwf 0x20,f\naddwf 0x20,0\naddwf 0x20,1\naddwf 0x20\nmovf 0x20")).unwrap();
    use pypic_pic14::Dest::{F, W};
    let want = [Inst::Addwf(0x20, W), Inst::Addwf(0x20, W), Inst::Addwf(0x20, F), Inst::Addwf(0x20, W), Inst::Addwf(0x20, F), Inst::Addwf(0x20, F), Inst::Movf(0x20, F)];
    for (i, inst) in want.iter().enumerate() {
        assert_eq!(w(&a, i as u32), enc(*inst), "#{i}");
    }
    assert_eq!(assemble(&abs("addwf 0x20,x")).unwrap_err()[0].code, "ASM003");
}

// ---- psects y enlace ---------------------------------------------------------------------------------

#[test]
fn abs_psects_start_at_zero_and_org_moves_inside_them() {
    let a = assemble("PSECT t,class=CODE,delta=2,abs,ovrld\n    nop\n    ORG 0x10\nx:  nop\n").unwrap();
    assert_eq!(a.symbols["x"], 0x10);
    assert!(a.image.get(0x10).is_some());
}

#[test]
fn org_is_relative_to_the_psect_and_cannot_go_backwards() {
    let e = env_with(&["-pmain=0x100"]);
    let a = ok_with("PSECT main,class=CODE,delta=2\n    nop\n    ORG 5\nx:  nop\n", &e);
    assert_eq!(a.symbols["x"], 0x105);
    assert_eq!(assemble("PSECT t,class=CODE,delta=2,abs\n  nop\n  nop\n  ORG 1\n").unwrap_err()[0].code, "ASM006");
}

#[test]
fn relocatable_code_goes_to_the_first_free_hole_of_its_class() {
    // -p fija el vector de reset y el de interrupcion; el resto de codigo se coloca despues, en el primer hueco que le cabe
    let e = env_with(&["-presetVec=0h", "-pintentry=4h"]);
    let src = "#include <xc.inc>\nPSECT resetVec,class=CODE,delta=2\n goto main\nPSECT intentry,class=CODE,delta=2\n retfie\nPSECT code\nmain: nop\n nop\n nop\n nop\n nop\n";
    let a = ok_with(src, &e);
    assert_eq!((a.symbols["main"], w(&a, 4)), (5, enc(Inst::Retfie)));
    assert_eq!(w(&a, 0), enc(Inst::Goto(5)));
}

#[test]
fn a_small_psect_fills_the_hole_between_the_vectors() {
    let e = env_with(&["-presetVec=0h", "-pintentry=4h"]);
    let src = "#include <xc.inc>\nPSECT resetVec,class=CODE,delta=2\n goto start\nPSECT intentry,class=CODE,delta=2\n retfie\nPSECT code\nstart: nop\n nop\n";
    // 2 palabras caben en el hueco 1..=3
    assert_eq!(ok_with(src, &e).symbols["start"], 1);
}

#[test]
fn the_default_reset_vec_psects_follow_the_official_linker_defaults() {
    // -preset_vec=00h,intentry,init,end_init
    let a = ok("PSECT intentry,class=CODE,delta=2\nisr: retfie\nPSECT reset_vec,class=CODE,delta=2\nrv: goto isr\n");
    assert_eq!((a.symbols["rv"], a.symbols["isr"]), (0, 1));
}

#[test]
fn data_psects_get_ram_addresses_from_their_bank_class() {
    let a = ok("#include <xc.inc>\nPSECT udata_bank0\na: DS 2\nb: DS 1\nPSECT udata_bank1\nc: DS 4\nPSECT udata_shr\nd: DS 1\nPSECT code\nx: nop\n");
    assert_eq!((a.symbols["a"], a.symbols["b"], a.symbols["c"], a.symbols["d"]), (0x20, 0x22, 0xA0, 0x70));
    // un psect de varias partes con el mismo nombre se concatena
    let a = ok("#include <xc.inc>\nPSECT udata_bank0\na: DS 1\nPSECT code\nx: nop\nPSECT udata_bank0\nb: DS 1\n");
    assert_eq!((a.symbols["a"], a.symbols["b"]), (0x20, 0x21));
}

#[test]
fn psect_errors() {
    assert!(msg("    nop\n").contains("fuera de un PSECT"));
    // un psect sin clase ni -p
    assert!(msg("PSECT lost,delta=2\n nop\n").contains("no tiene clase"));
    // un psect que no cabe en una pagina de codigo
    let big = format!("PSECT code2,class=CODE,delta=2\n ORG 0x800\n nop\n");
    assert!(msg(&big).contains("no cabe"), "{}", msg(&big));
    // dos psects fijos que se solapan
    let e = env_with(&["-pa=0h", "-pb=0h"]);
    let r = assemble_with("PSECT a,class=CODE,delta=2\n nop\nPSECT b,class=CODE,delta=2\n nop\n", &e).unwrap_err();
    assert!(r[0].message.contains("se solapa"), "{r:?}");
    // clase desconocida
    assert_eq!(code("PSECT p,class=NOPE,delta=2\n"), "ASM010");
    // codigo en un psect de datos
    assert!(msg("#include <xc.inc>\nPSECT udata_bank0\n nop\n").contains("de datos"));
    // sin xc.inc los psects predefinidos no existen
    assert!(msg("PSECT code\n nop\n").contains("no tiene clase"));
}

#[test]
fn link_options_are_parsed_like_the_official_driver() {
    let mut e = env();
    e.add_link_options(&["-presetVec=0h".into(), "-pintentry=4h,init".into(), "--fixupoverflow=ignore".into()]).unwrap();
    assert_eq!(e.placements, vec![("resetVec".to_string(), 0, vec![]), ("intentry".to_string(), 4, vec!["init".to_string()])]);
    assert!(e.fixup_ignore);
    assert!(e.add_link_options(&["-Ynope".into()]).is_err());
}

// ---- datos -------------------------------------------------------------------------------------------

#[test]
fn db_and_dw_write_one_program_word_per_value_with_delta_2() {
    let a = ok(&abs("DB 'X',1,2,3\nDW 398,5472,0x3FFF\nDB \"ab\",0"));
    let words: Vec<u16> = (0..10).map(|i| w(&a, i)).collect();
    assert_eq!(words, vec![0x58, 1, 2, 3, 398, 5472, 0x3FFF, 0x61, 0x62, 0]);
    assert_eq!(assemble(&abs("DW 0x4000")).unwrap_err()[0].code, "ASM004");
    assert_eq!(assemble(&abs("DB 256")).unwrap_err()[0].code, "ASM004");
}

#[test]
fn equ_set_and_forward_references() {
    let a = ok(&abs("BASE EQU 0x20\nNEXT EQU BASE+1\nTOP EQU end_label+1\n    movwf NEXT\nend_label: nop\nV SET 3\n    movlw V\nV SET 7\n    movlw V\n"));
    assert_eq!(a.symbols["NEXT"], 0x21);
    // una constante puede apoyarse en una etiqueta que se define despues
    assert_eq!(a.symbols["TOP"], 2);
    assert_eq!(w(&a, 0), enc(Inst::Movwf(0x21)));
    assert_eq!((w(&a, 2), w(&a, 3)), (enc(Inst::Movlw(3)), enc(Inst::Movlw(7))));
    assert_eq!(code("A EQU 1\nA EQU 2\n"), "ASM005");
}

#[test]
fn ds_in_a_program_psect_only_moves_the_location_counter() {
    let a = ok(&abs("a: nop\n DS 3\nb: nop"));
    assert_eq!(a.symbols["b"], 4);
    assert!(a.image.get(2).is_none());
}

// ---- xc.inc ------------------------------------------------------------------------------------------

#[test]
fn xc_inc_provides_registers_fields_and_position_symbols() {
    let a = ok("#include <xc.inc>\nPSECT code,abs\n    bsf STATUS,STATUS_RP0_POSN\n    btfss RB5\n    movlw PORTB_RB5_MASK\n    movlw INTCON_GIE_POSITION\n");
    assert_eq!(w(&a, 0), enc(Inst::Bsf(0x03, 5)));
    assert_eq!(w(&a, 1), enc(Inst::Btfss(0x06, 5))); // RB5 -> PORTB,5
    assert_eq!(w(&a, 2), enc(Inst::Movlw(0x20)));
    assert_eq!(w(&a, 3), enc(Inst::Movlw(7)));
}

#[test]
fn xc_inc_needs_a_device_and_nothing_else_can_be_included() {
    let e = assemble("#include <xc.inc>\n").unwrap_err();
    assert!(e[0].message.contains("dispositivo"), "{e:?}");
    assert!(msg("#include <otro.inc>\n").contains("xc.inc"));
}

#[test]
fn processor_must_match_the_device() {
    assert_eq!(code("PROCESSOR 16F877A\n"), "ASM011");
    ok("PROCESSOR 16F887\n");
    ok("PROCESSOR PIC16F887\n");
    assert_eq!(pypic_pic14::declared_processor("; x\n  processor 16F887\n").as_deref(), Some("16F887"));
}

// ---- preprocesador y condicionales -------------------------------------------------------------------

#[test]
fn define_ifdef_and_if_work() {
    let a = ok(&abs("#define K 7\n#ifdef K\n movlw K\n#else\n movlw 1\n#endif\n#ifndef K\n movlw 2\n#endif\n IF 1\n movlw 3\n ELSE\n movlw 4\n ENDIF\n IF 0\n movlw 5\n ELSIF 1\n movlw 6\n ELSE\n movlw 8\n ENDIF"));
    let got: Vec<u16> = (0..3).map(|i| w(&a, i)).collect();
    assert_eq!(got, vec![enc(Inst::Movlw(7)), enc(Inst::Movlw(3)), enc(Inst::Movlw(6))]);
    assert!(a.image.get(3).is_none());
}

// ---- BANKSEL, PAGESEL, fcall, ljmp -------------------------------------------------------------------

#[test]
fn banksel_selects_the_bank_of_a_symbol_in_two_words() {
    let a = ok("#include <xc.inc>\nPSECT code,abs\n BANKSEL TRISB\n BANKSEL(ANSEL)\n BANKSEL PORTB\n");
    // TRISB = 0x86 (banco 1), ANSEL = 0x188 (banco 3), PORTB = 0x06 (banco 0)
    assert_eq!((w(&a, 0), w(&a, 1)), (enc(Inst::Bsf(3, 5)), enc(Inst::Bcf(3, 6))));
    assert_eq!((w(&a, 2), w(&a, 3)), (enc(Inst::Bsf(3, 5)), enc(Inst::Bsf(3, 6))));
    assert_eq!((w(&a, 4), w(&a, 5)), (enc(Inst::Bcf(3, 5)), enc(Inst::Bcf(3, 6))));
}

#[test]
fn fcall_and_ljmp_set_pclath_and_restore_the_page() {
    let e = env_with(&["-pfar=0x800"]);
    let src = "PSECT far,class=CODE,delta=2\nfn: return\nPSECT t,class=CODE,delta=2,abs\n fcall fn\n ljmp fn\n PAGESEL fn\n PAGESEL $\n";
    let a = ok_with(src, &e);
    // fcall: bcf PCLATH,4 ; bsf PCLATH,3 ; call ; bcf PCLATH,4 ; bcf PCLATH,3 (el ejemplo de la guia)
    let want = [Inst::Bcf(0x0A, 4), Inst::Bsf(0x0A, 3), Inst::Call(0), Inst::Bcf(0x0A, 4), Inst::Bcf(0x0A, 3)];
    for (i, inst) in want.iter().enumerate() {
        assert_eq!(w(&a, i as u32), enc(*inst), "fcall #{i}");
    }
    assert_eq!((w(&a, 5), w(&a, 6), w(&a, 7)), (enc(Inst::Bcf(0x0A, 4)), enc(Inst::Bsf(0x0A, 3)), enc(Inst::Goto(0))));
    assert_eq!((w(&a, 8), w(&a, 9)), (enc(Inst::Bsf(0x0A, 3)), enc(Inst::Bcf(0x0A, 4)))); // PAGESEL fn: pagina 1
    assert_eq!((w(&a, 10), w(&a, 11)), (enc(Inst::Bcf(0x0A, 3)), enc(Inst::Bcf(0x0A, 4)))); // PAGESEL $: pagina 0
}

#[test]
fn operands_that_do_not_fit_need_bankmask_or_pagemask() {
    let m = msg("#include <xc.inc>\nPSECT code,abs\n movwf TRISB\n");
    assert!(m.contains("BANKMASK") && m.contains("BANKSEL"), "{m}");
    let a = ok("#include <xc.inc>\nPSECT code,abs\n BANKSEL TRISB\n movwf BANKMASK(TRISB)\n");
    assert_eq!(w(&a, 2), enc(Inst::Movwf(0x06)));
    let m = msg("PSECT far,class=CODE,delta=2,abs\n ORG 0x900\nx: nop\nPSECT t,class=CODE,delta=2\n call x\n");
    assert!(m.contains("PAGEMASK") && m.contains("fcall"), "{m}");
    let a = assemble_with(&format!("{}\nORG 0x900\nx: nop\n call PAGEMASK(x)\n", "PSECT t,class=CODE,delta=2,abs"), &env()).unwrap();
    assert_eq!(w(&a, 0x901), enc(Inst::Call(0x100)));
    // --fixupoverflow=ignore trunca como pic-as
    let e = env_with(&["--fixupoverflow=ignore"]);
    let a = ok_with("#include <xc.inc>\nPSECT code,abs\n movwf TRISB\n", &e);
    assert_eq!(w(&a, 0), enc(Inst::Movwf(0x06)));
}

// ---- CONFIG ------------------------------------------------------------------------------------------

#[test]
fn config_settings_use_the_names_of_the_dfp_and_start_from_the_default_word() {
    let a = ok("CONFIG FOSC = INTRC_NOCLKOUT, WDTE = OFF, PWRTE = OFF, MCLRE = OFF, CP = OFF, CPD = OFF\nCONFIG BOREN = OFF\nCONFIG IESO = OFF, FCMEN = OFF, LVP = OFF, DEBUG = OFF\n");
    assert_eq!(a.image.get(0x2007), Some(0x20D4));
    assert_eq!(a.image.get(0x2008), None); // CONFIG2 no se tocó
    let a = ok("CONFIG BOR4V = BOR21V, WRT = HALF\n");
    assert_eq!(a.image.get(0x2008), Some(0x3FFF & !(1 << 8) & !(3 << 9)));
}

#[test]
fn config_accepts_quotes_numbers_and_whole_registers() {
    let a = ok("CONFIG \"WDTE = ON\"\nCONFIG \"FOSC\" = \"XT\"\nCONFIG WRT = 2\nCONFIG CONFIG2 = 0x1234\n");
    // CONFIG2 se fija entera al final de las instrucciones; WRT se aplica sobre el valor por defecto antes de eso
    assert_eq!(a.image.get(0x2007), Some(0x3FFF & !(7) | 1));
    assert!(a.image.get(0x2008).is_some());
}

#[test]
fn config_accepts_digit_only_value_name_without_reinterpreting_hex_literals() {
    let a = ok("CONFIG WRT = 256\n");
    assert_eq!(a.image.get(0x2008), Some(0x3dff));
    assert!(a.warnings.is_empty());
    let a = ok("CONFIG WRT = \"256\"\n");
    assert_eq!(a.image.get(0x2008), Some(0x3dff));
    let a = ok("CONFIG WRT = 0x100\n");
    assert_eq!(a.image.get(0x2008), Some(0x39ff));
    assert_eq!(a.warnings[0].code, "ASMW001");
}

#[test]
fn id_locations_are_explicit_masked_and_unique() {
    let a = ok("CONFIG IDLOC0 = 0x41, IDLOC1 = 0, IDLOC2 = 0x20, IDLOC3 = 0x7f\n");
    assert_eq!(a.image.get(0x2000), Some(0x41));
    assert_eq!(a.image.get(0x2001), Some(0));
    assert_eq!(a.image.get(0x2002), Some(0x20));
    assert_eq!(a.image.get(0x2003), Some(0x7f));
    assert_eq!(ok("CONFIG IDLOC0 = 1\n").image.get(0x2001), None);
    assert_eq!(a.image.get(0x2007), None);
    let truncated = ok("CONFIG IDLOC0 = 0x80\nCONFIG CONFIG1 = 0x4000\n");
    assert_eq!(truncated.image.get(0x2000), Some(0));
    assert_eq!(truncated.image.get(0x2007), Some(0));
    assert_eq!(truncated.warnings.len(), 2);
    assert!(truncated.warnings.iter().all(|w| w.code == "ASMW001"));
    assert_eq!(code("CONFIG IDLOC0 = 1, IDLOC0 = 2\n"), "ASM013");
    assert_eq!(code("CONFIG DEVID = 1\n"), "ASM013");
}

#[test]
fn config_errors_list_the_valid_choices() {
    let m = msg("CONFIG WDTE = MAYBE\n");
    assert!(m.contains("ON") && m.contains("OFF"), "{m}");
    let m = msg("CONFIG NOPE = ON\n");
    assert!(m.contains("FOSC") && m.contains("no es un ajuste"), "{m}");
    assert_eq!(code("CONFIG WDTE = OFF\nCONFIG WDTE = ON\n"), "ASM013");
    assert_eq!(ok("CONFIG WDTE = 2\n").image.get(0x2007), Some(0x3ff7));
    assert_eq!(assemble("CONFIG WDTE = OFF\n").unwrap_err()[0].code, "ASM013"); // sin dispositivo
}

// ---- mensajes que guian desde MPASM -------------------------------------------------------------------

#[test]
fn unknown_and_foreign_instructions_explain_themselves() {
    assert_eq!(code(&abs("    FOO 1")), "ASM002");
    let e = errs(&abs("   FOO 1"));
    assert_eq!((e[0].line, e[0].col), (2, 4));
    let m = msg(&abs("    MOVLB 1"));
    assert!(m.contains("PIC16F1xxx") && m.contains("no existe en el PIC16 clasico"), "{m}");
    let m = msg(&abs("    SETC"));
    assert!(m.contains("MPASM") && m.contains("bsf STATUS,0"), "{m}");
    let m = msg(&abs("    MOVFW 0x20"));
    assert!(m.contains("movf f,w"), "{m}");
}

#[test]
fn mpasm_directives_say_how_pic_as_writes_them() {
    let m = msg("__CONFIG 0x20D4\n");
    assert!(m.contains("CONFIG nombre = valor"), "{m}");
    assert!(msg("CBLOCK 0x20\n").contains("DS"));
    assert!(msg("    RES 2\n").contains("DS n"));
    let m = msg(&abs("loop\n    goto loop"));
    assert!(m.contains("termina en ':'"), "{m}");
    assert!(msg("m MACRO\n").contains("falta el ENDM"));
}

// ---- errores -------------------------------------------------------------------------------------------

#[test]
fn operand_ranges_are_checked() {
    assert_eq!(code(&abs("    BSF 0x03,8")), "ASM004");
    assert_eq!(code(&abs("    MOVLW 256")), "ASM004");
    assert_eq!(code(&abs("    MOVLW -1")), "ASM004");
}

#[test]
fn symbol_errors() {
    assert_eq!(code(&abs("    GOTO nowhere")), "ASM005");
    assert!(msg(&abs("    GOTO nowhere")).contains("'nowhere'"));
    assert_eq!(code(&abs("a: NOP\na: NOP")), "ASM005");
    assert_eq!(code(&abs("nop: NOP")), "ASM005"); // palabra reservada
    assert_eq!(code("W EQU 1\n"), "ASM005");
}

#[test]
fn operand_count_and_syntax_errors() {
    assert_eq!(code(&abs("    NOP 1")), "ASM003");
    assert_eq!(code(&abs("    MOVLW")), "ASM003");
    assert_eq!(code(&abs("a: NOP\n    GOTO a,a")), "ASM003");
    assert_eq!(code(&abs("    MOVLW #5")), "ASM001");
    assert_eq!(code(&abs("    MOVLW 5 +")), "ASM003");
    assert_eq!(code(&abs("    MOVLW 12abc")), "ASM001");
}

#[test]
fn overlapping_addresses_are_rejected() {
    let e = errs("PSECT a,class=CODE,delta=2,abs\n    nop\nPSECT b,class=CODE,delta=2,abs\n    nop\n");
    assert!(e.iter().any(|x| x.code == "ASM006"), "{e:?}");
}

#[test]
fn all_errors_are_reported_in_source_order_with_the_rendered_format() {
    let e = errs(&abs("    FOO\n    MOVLW 999\n    GOTO nowhere"));
    assert_eq!(e.iter().map(|x| (x.line, x.code)).collect::<Vec<_>>(), vec![(2, "ASM002"), (3, "ASM004"), (4, "ASM005")]);
    let out = render_errors(&e, "p.s");
    assert!(out.starts_with("p.s:2:5: error[ASM002]"), "{out}");
}

#[test]
fn listing_shows_address_word_and_source() {
    let a = ok(&abs("    movlw 0x2A ; k\nx:  nop"));
    assert!(a.listing.contains("0000 302A      movlw 0x2A ; k"), "{}", a.listing);
    assert!(a.listing.contains("0001 0000  x:  nop"), "{}", a.listing);
}

#[test]
fn end_stops_the_assembly() {
    let a = ok(&abs("    nop\n    END\n    this is not even assembly"));
    assert_eq!(a.image.len(), 1);
}
