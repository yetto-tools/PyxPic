//! Unidades en los parametros de los perifericos (`UART.start(baud=9600baud)`, `pause(500ms)`, `PWM1.start(frequency=20kHz, duty=50pct)`,
//! `TMR2.every(20ms)`, `ADC.start(reference=5V)`): la conversion es del compilador y el firmware generado es **identico** al de la forma con
//! enteros de siempre; una magnitud que no corresponde al parametro es un error con su codigo.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

const HEAD: &str = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.uart import UART\nfrom pyxpic.periph.spi import SPI\nfrom pyxpic.periph.i2c import I2C\nfrom pyxpic.periph.pwm import PWM1\nfrom pyxpic.periph.adc import ADC\nfrom pyxpic.periph.timer import TMR2\nfrom pyxpic.system.time import pause, wait, delay_us, delay_cycles\nCLOCK: Internal\nFREQUENCY: 4MHz\n";

fn program(body: &str) -> String {
    format!("{HEAD}\n\ndef main() -> None:\n    {body}\n")
}

/// Compila y devuelve las palabras de programa; `Err` con los diagnosticos si el frontend rechaza el programa.
fn words(src: &str) -> Result<Vec<(u32, u16)>, String> {
    let r = analyze(src, &pack().target_profile());
    let p = match r.program {
        Some(p) => p,
        None => return Err(r.diagnostics.render(src, "t.py")),
    };
    let m = lower(&p);
    verify(&m).unwrap();
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    Ok(c.image.iter().collect())
}

fn same(a: &str, b: &str) {
    let (wa, wb) = (words(&program(a)).unwrap_or_else(|e| panic!("{a}: {e}")), words(&program(b)).unwrap_or_else(|e| panic!("{b}: {e}")));
    assert_eq!(wa, wb, "'{a}' y '{b}' deben generar el mismo firmware");
}

fn rejected(body: &str, code: &str, frag: &str) {
    let e = words(&program(body)).expect_err(body);
    assert!(e.contains(code) && e.contains(frag), "{body}: {e}");
}

#[test]
fn a_unit_literal_generates_exactly_the_firmware_of_the_plain_integer() {
    same("UART.start(baud=9600baud)", "UART.start(baud=9600)");
    same("UART.start(baud=9.6kbaud)", "UART.start(baud=9600)");
    same("I2C.start(frequency=100kHz)", "I2C.start(freq=100000)");
    same("SPI.start(frequency=1MHz)", "SPI.start(freq=1000000)");
    same("SPI.start(frequency=500kHz, mode=1)", "SPI.start(freq=500000, mode=1)");
    same("pause(500ms)", "pause(500)");
    same("pause(2s)", "pause(2000)");
    same("wait(2s)", "wait(2)");
    same("delay_us(250us)", "delay_us(250)");
    same("delay_us(1ms)", "delay_us(1000)");
    same("ADC.start(acquisition_us=20us)", "ADC.start(acquisition_us=20)");
    same("PWM1.start(frequency=20kHz)", "PWM1.start(frequency=20000)");
}

#[test]
fn a_percent_duty_is_a_share_of_the_period_resolved_at_compile_time() {
    // a 4 MHz y 20 kHz el periodo son 200 cuentas (4 x (PR2 + 1) con PR2 = 49): 50 % son 100 y 25 % son 50
    same("PWM1.start(frequency=20kHz, duty=50pct)", "PWM1.start(frequency=20000, duty=100)");
    same("PWM1.start(frequency=20kHz, duty=25pct)", "PWM1.start(frequency=20000, duty=50)");
    same("PWM1.start(frequency=20kHz, duty=0pct)", "PWM1.start(frequency=20000, duty=0)");
    same("PWM1.start(frequency=20kHz, duty=12.5pct)", "PWM1.start(frequency=20000, duty=25)");
    rejected("PWM1.start(frequency=20kHz, duty=120pct)", "PYP2241", "de 0pct a 100pct");
    rejected("PWM1.start(frequency=20kHz, duty=5V)", "PYP2240", "porcentaje");
}

#[test]
fn tmr2_every_solves_prescaler_pr2_and_postscaler_exactly() {
    // 20 ms a 4 MHz son 20 000 ciclos; el resolvedor elige el menor prescaler y el menor postscaler que lo forman exactamente
    let st = pyxpic_frontend::timer::solve_t2(20_000).unwrap();
    let raw = format!("TMR2.start(prescaler={}, period={}, postscaler={})", st.prescaler, st.period, st.postscaler);
    same("TMR2.every(20ms)", &raw);
    let st = pyxpic_frontend::timer::solve_t2(1_000).unwrap();
    same("TMR2.every(1ms)", &format!("TMR2.start(prescaler={}, period={}, postscaler={})", st.prescaler, st.period, st.postscaler));
    // un tiempo que no es un numero entero de ciclos (un ciclo son 1000 ns a 4 MHz)
    rejected("TMR2.every(1500ns)", "PYP2243", "numero entero de ciclos");
    // un periodo que Timer2 no alcanza: 16 x 256 x 16 = 65 536 ciclos como maximo
    rejected("TMR2.every(100ms)", "PYP2243", "mas cercanos");
    rejected("TMR2.every(20)", "PYP2240", "tiempo con unidad");
    rejected("TMR2.every(5V)", "PYP2240", "tiempo con unidad");
}

#[test]
fn a_magnitude_that_does_not_fit_the_parameter_is_an_error_with_its_code() {
    rejected("UART.start(baud=9600Hz)", "PYP2240", "espera baudios");
    rejected("UART.start(baud=1ms)", "PYP2240", "espera baudios");
    rejected("I2C.start(frequency=3V)", "PYP2240", "espera frecuencia");
    rejected("SPI.start(frequency=10ms)", "PYP2240", "espera frecuencia");
    rejected("pause(500Hz)", "PYP2240", "espera tiempo");
    rejected("delay_cycles(5ms)", "PYP2240", "ciclos de instruccion");
    rejected("PWM1.start(frequency=20ms)", "PYP2240", "espera frecuencia");
    // un valor que no es entero en la unidad del parametro
    rejected("pause(1500us)", "PYP2241", "numero entero de ms");
    rejected("pause(0.0005s)", "PYP2241", "numero entero de ms");
    same("pause(0.5s)", "pause(500)");
    rejected("wait(500ms)", "PYP2241", "numero entero de s");
    // un tiempo suelto (sin parametro) sigue sin significado
    rejected("x: uint32 = 500ms", "PYP2230", "tiempo");
}

#[test]
fn adc_reference_decides_low_voltage_and_is_checked_against_the_supply_of_the_device() {
    same("ADC.start(reference=5V)", "ADC.start()");
    same("ADC.start(reference=3.3V)", "ADC.start(low_voltage=False)");
    same("ADC.start(reference=2.5V)", "ADC.start(low_voltage=True)");
    same("ADC.start(reference=2V)", "ADC.start(low_voltage=True)");
    rejected("ADC.start(reference=6V)", "PYP2242", "fuera de la alimentacion");
    rejected("ADC.start(reference=1.8V)", "PYP2242", "2 V a 5.5 V");
    rejected("ADC.start(reference=5)", "PYP2240", "tension con unidad");
    rejected("ADC.start(reference=5Hz)", "PYP2240", "espera voltaje");
    rejected("ADC.start(reference=2.5V, low_voltage=False)", "PYP2172", "contradice reference");
}
