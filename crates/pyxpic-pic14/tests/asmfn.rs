//! Funciones `@asm_function`: ensamblador dentro del lenguaje, con parametros y globales por nombre.

use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify, Machine, Trap};
use pyxpic_pic14::codegen::{CodegenError, Compiled, Options};
use pyxpic_pic14::compile;
use pyxpic_pic14::sim::Sim;

fn try_build(src: &str) -> Result<Compiled, CodegenError> {
    let r = analyze(src, &TargetProfile::generic());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    compile(&m, &Options::default())
}

fn run(src: &str) -> (Compiled, Sim) {
    let c = try_build(src).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    let mut s = Sim::new(&c.image);
    s.run_until(10_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    (c, s)
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u16 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let lo = s.read(g.addr as usize) as u16;
    if g.ty.bits() == 16 {
        lo | (s.read(g.addr as usize + 1) as u16) << 8
    } else {
        lo
    }
}

fn frontend_codes(src: &str) -> Vec<String> {
    let r = analyze(src, &TargetProfile::generic());
    r.diagnostics.render(src, "t.py").lines().filter_map(|l| l.split("error[").nth(1).map(|x| x[..7].to_string())).collect()
}

const HEAD: &str = "from pyxpic import u8, u16, asm_function\n\n";

#[test]
fn an_asm_function_reads_its_parameter_by_name_and_returns_in_w() {
    let src = format!(
        "{HEAD}result: u8 = 0\n\n\n@asm_function\ndef swap(x: u8) -> u8:\n    \"\"\"\n    SWAPF x,W\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    global result\n    result = swap(0xA5)\n"
    );
    let (c, s) = run(&src);
    assert_eq!(global(&c, &s, "result"), 0x5A);
}

#[test]
fn sixteen_bit_results_use_retv_for_the_high_byte() {
    let src = format!(
        "{HEAD}out: u16 = 0\n\n\n@asm_function\ndef add3(x: u16) -> u16:\n    \"\"\"\n    MOVF x+1,W\n    MOVWF RETV\n    MOVLW 3\n    ADDWF x,W\n    BTFSC STATUS,STATUS_C_POSN\n    INCF RETV,F\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    global out\n    out = add3(0x01FE)\n"
    );
    let (c, s) = run(&src);
    assert_eq!(global(&c, &s, "out"), 0x0201);
}

#[test]
fn labels_dollar_and_globals_by_name_work_like_in_the_textual_assembler() {
    let src = format!(
        "{HEAD}hits: u8 = 0\nmarker: u8 = 0\n\n\n@asm_function\ndef spin(n: u8) -> u8:\n    \"\"\"\n    MOVF n,F          ; Z=1 si n es 0\n    BTFSC STATUS,STATUS_Z_POSN\n    GOTO done\nagain:\n    INCF hits,F\n    DECFSZ n,F\n    GOTO again\ndone:\n    MOVLW 0x55\n    MOVWF marker\n    GOTO $+1\n    RETLW 7\n    \"\"\"\n\n\ndef main() -> None:\n    r: u8 = spin(5)\n    global marker\n    marker = marker + r\n"
    );
    let (c, s) = run(&src);
    assert_eq!(global(&c, &s, "hits"), 5);
    assert_eq!(global(&c, &s, "marker"), 0x55 + 7);
}

#[test]
fn banks_other_than_zero_are_reachable_when_the_function_returns_to_bank_zero() {
    // TRISD vive en el banco 1: se lee y se vuelve al banco 0, como pide la ABI
    let src = format!(
        "{HEAD}tris: u8 = 0\n\n\n@asm_function\ndef read_trisd() -> u8:\n    \"\"\"\n    BSF STATUS,STATUS_RP0_POSN\n    MOVF BANKMASK(TRISD),W\n    BCF STATUS,STATUS_RP0_POSN\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    global tris\n    tris = read_trisd()\n"
    );
    let (c, s) = run(&src);
    assert_eq!(global(&c, &s, "tris"), 0xFF); // tras el reset todos los TRIS valen 1
}

#[test]
fn the_inline_assembly_words_are_in_the_final_image_and_the_listing() {
    let src = format!("{HEAD}r: u8 = 0\n\n\n@asm_function\ndef swap(x: u8) -> u8:\n    \"\"\"\n    SWAPF x,W\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    global r\n    r = swap(1)\n");
    let c = try_build(&src).unwrap();
    assert!(c.listing.contains("swap:\n") && c.listing.contains("SWAPF"), "{}", c.listing);
}

// ---- errores del ensamblador, con la linea del fuente ---------------------------------------------

fn asm_errors(body: &str) -> Vec<(usize, &'static str, String)> {
    let src = format!("{HEAD}@asm_function\ndef f() -> None:\n    \"\"\"\n{body}    \"\"\"\n\n\ndef main() -> None:\n    f()\n");
    match try_build(&src) {
        Err(CodegenError::Asm(e)) => e.into_iter().map(|x| (x.line, x.code, x.message)).collect(),
        other => panic!("esperaba errores de ensamblador, salio {:?}\n{src}", other.map(|_| ())),
    }
}

#[test]
fn errors_point_at_the_line_inside_the_source_file() {
    // linea 1-2: imports; 3 decorador; 4 def; 5 abre las comillas; 6 es la primera linea del ensamblador
    let e = asm_errors("    NOP\n    FOO\n    RETURN\n");
    assert_eq!((e[0].0, e[0].1), (7, "ASM002"), "{e:?}");
}

#[test]
fn returning_with_a_bank_selected_is_rejected() {
    let e = asm_errors("    BSF STATUS,STATUS_RP0_POSN\n    MOVF BANKMASK(TRISD),W\n    RETURN\n");
    assert!(e[0].1 == "ASM007" && e[0].2.contains("RP0/RP1"), "{e:?}");
    // restablecer un solo bit no basta
    let e = asm_errors("    BSF STATUS,STATUS_RP0_POSN\n    BSF STATUS,STATUS_RP1_POSN\n    BCF STATUS,STATUS_RP0_POSN\n    RETURN\n");
    assert_eq!(e[0].1, "ASM007");
}

#[test]
fn a_function_must_end_in_return_retlw_or_goto() {
    let e = asm_errors("    NOP\n    NOP\n");
    assert!(e[0].1 == "ASM007" && e[0].2.contains("RETURN"), "{e:?}");
    let e = asm_errors("");
    assert!(e[0].2.contains("no tiene instrucciones"), "{e:?}");
}

#[test]
fn org_dw_retfie_and_enhanced_instructions_are_rejected() {
    assert_eq!(asm_errors("    ORG 0x10\n    RETURN\n")[0].1, "ASM007");
    assert_eq!(asm_errors("    DW 1\n    RETURN\n")[0].1, "ASM007");
    assert!(asm_errors("    RETFIE\n")[0].2.contains("RETURN o RETLW"));
    assert_eq!(asm_errors("    MOVLB 1\n    RETURN\n")[0].1, "ASM002");
}

#[test]
fn jumps_stay_inside_the_function_and_target_labels_only() {
    assert_eq!(asm_errors("    GOTO nowhere\n    RETURN\n")[0].1, "ASM005");
    assert_eq!(asm_errors("    GOTO $+5\n    RETURN\n")[0].1, "ASM006");
    let e = asm_errors("    CALL STATUS\n    RETURN\n");
    assert!(e[0].1 == "ASM005" && e[0].2.contains("es un dato"), "{e:?}");
}

#[test]
fn a_name_that_clashes_with_a_predefined_register_is_reported() {
    let src = format!("{HEAD}@asm_function\ndef f(STATUS: u8) -> None:\n    \"\"\"\n    BSF STATUS,STATUS_Z_POSN\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    f(1)\n");
    match try_build(&src) {
        Err(CodegenError::Asm(e)) => assert!(e[0].message.contains("STATUS") && e[0].message.contains("renombra"), "{e:?}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn the_error_text_names_the_source_line() {
    let src = format!("{HEAD}@asm_function\ndef f() -> None:\n    \"\"\"\n    FOO\n    \"\"\"\n\n\ndef main() -> None:\n    f()\n");
    let msg = try_build(&src).err().unwrap().to_string();
    assert!(msg.contains("linea 6:5: error[ASM002]"), "{msg}");
}

// ---- reglas del lenguaje --------------------------------------------------------------------------

#[test]
fn the_body_must_be_one_string() {
    let c = frontend_codes(&format!("{HEAD}@asm_function\ndef f() -> None:\n    pass\n\n\ndef main() -> None:\n    f()\n"));
    assert!(c.contains(&"PYP2110".to_string()), "{c:?}");
    let c = frontend_codes(&format!("{HEAD}@asm_function\ndef f() -> None:\n    \"\"\"\n    RETURN\n    \"\"\"\n    pass\n\n\ndef main() -> None:\n    f()\n"));
    assert!(c.contains(&"PYP2110".to_string()), "{c:?}");
}

#[test]
fn strings_are_still_rejected_anywhere_else() {
    let c = frontend_codes(&format!("{HEAD}def main() -> None:\n    x = \"hola\"\n"));
    assert!(c.contains(&"PYP2164".to_string()), "{c:?}");
}

#[test]
fn an_asm_function_cannot_be_reached_from_an_interrupt() {
    let src = format!(
        "{HEAD}from pyxpic import interrupt\n\n\n@asm_function\ndef f() -> None:\n    \"\"\"\n    RETURN\n    \"\"\"\n\n\n@interrupt\ndef isr() -> None:\n    f()\n\n\ndef main() -> None:\n    pass\n"
    );
    assert!(frontend_codes(&src).contains(&"PYP2111".to_string()));
}

#[test]
fn parameters_and_results_are_limited_to_sixteen_bits() {
    let src = format!("{HEAD}from pyxpic import u32\n\n\n@asm_function\ndef f(x: u32) -> None:\n    \"\"\"\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    pass\n");
    assert!(frontend_codes(&src).contains(&"PYP2112".to_string()));
}

#[test]
fn the_reference_interpreter_does_not_run_asm_functions() {
    let src = format!("{HEAD}def_unused: u8 = 0\n\n\n@asm_function\ndef f() -> None:\n    \"\"\"\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    f()\n");
    let r = analyze(&src, &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    assert_eq!(Machine::new(&m).run_main().unwrap_err(), Trap::AsmFunction("f".into()));
}

#[test]
fn a_global_with_a_predefined_name_is_fine_when_the_block_does_not_use_it() {
    let src = format!("{HEAD}STATUS: u8 = 0\n\n\n@asm_function\ndef f() -> None:\n    \"\"\"\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    f()\n");
    assert!(try_build(&src).is_ok());
}

#[test]
fn bit_names_from_the_register_map_work_in_the_block() {
    let src = format!("{HEAD}@asm_function\ndef start() -> None:\n    \"\"\"\n    BSF GO_DONE\n    BSF ADCON0,ADCON0_GO_DONE_POSN\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    start()\n");
    let c = try_build(&src).unwrap();
    // el campo GO_DONE es una macro de xc.inc (ADCON0,1) y ADCON0_GO_DONE_POSN vale 1: las dos formas dan la misma instruccion
    assert_eq!(c.listing.matches("BSF 0x1f,1").count(), 2, "{}", c.listing);
}

#[test]
fn a_register_from_another_bank_needs_bankmask_like_in_pic_as() {
    let e = asm_errors("    BSF STATUS,STATUS_RP0_POSN\n    MOVF TRISD,W\n    BCF STATUS,STATUS_RP0_POSN\n    RETURN\n");
    assert!(e[0].1 == "ASM004" && e[0].2.contains("BANKMASK"), "{e:?}");
}
