//! Literales con unidad (`8MHz`, `11.0592MHz`, `500ms`, `3.3V`, `8e6Hz`, `50pct`): un sistema generico de sufijos.
//!
//! El lexer reconoce un numero decimal (con `_`, parte decimal y exponente) pegado a un sufijo de la tabla `UNITS`, y lo convierte a
//! un entero exacto en la **unidad base de su magnitud** (Hz, ns, B, uV...). No hay punto flotante: la conversion es aritmetica de
//! enteros de 128 bits y falla si el valor no es un numero entero de unidades base (`0.5Hz` no lo es). Las unidades no dependen de
//! ningun microcontrolador: que un dispositivo pueda generar o medir la magnitud lo valida cada funcion del SDK con su Device Pack.
//!
//! Los sufijos distinguen mayusculas y minusculas (`MHz` es frecuencia, `mH` es inductancia, `MB` es memoria, `mV` es voltaje).

use std::fmt;

/// La magnitud fisica de un literal. Dos valores solo se suman o comparan si tienen la misma.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dim {
    Frequency,
    Time,
    Memory,
    Baud,
    DataRate,
    Voltage,
    Current,
    Resistance,
    Capacitance,
    Inductance,
    Power,
    Temperature,
    Rotation,
    Angle,
    Percent,
}

impl Dim {
    /// Nombre de la magnitud, para los mensajes.
    pub fn name(self) -> &'static str {
        match self {
            Dim::Frequency => "frecuencia",
            Dim::Time => "tiempo",
            Dim::Memory => "memoria",
            Dim::Baud => "baudios",
            Dim::DataRate => "tasa de datos",
            Dim::Voltage => "voltaje",
            Dim::Current => "corriente",
            Dim::Resistance => "resistencia",
            Dim::Capacitance => "capacitancia",
            Dim::Inductance => "inductancia",
            Dim::Power => "potencia",
            Dim::Temperature => "temperatura",
            Dim::Rotation => "rotacion",
            Dim::Angle => "angulo",
            Dim::Percent => "porcentaje",
        }
    }

    /// La unidad en la que se guarda el valor entero.
    pub fn base_unit(self) -> &'static str {
        match self {
            Dim::Frequency => "Hz",
            Dim::Time => "ns",
            Dim::Memory => "B",
            Dim::Baud => "baud",
            Dim::DataRate => "bps",
            Dim::Voltage => "uV",
            Dim::Current => "uA",
            Dim::Resistance => "mohm",
            Dim::Capacitance => "pF",
            Dim::Inductance => "nH",
            Dim::Power => "uW",
            Dim::Temperature => "mK",
            Dim::Rotation => "rpm",
            Dim::Angle => "udeg",
            Dim::Percent => "mpct",
        }
    }

    /// `true` si su unidad base es la que el lenguaje ya usaba como entero simple (Hz en `CLOCK_HZ`, baudios en `UART.start(baud=9600)`,
    /// bytes): esos valores se pueden usar donde se espera un numero. Las demas magnitudes solo se aceptan donde un parametro las pide.
    pub fn is_plain_integer(self) -> bool {
        matches!(self, Dim::Frequency | Dim::Baud | Dim::DataRate | Dim::Memory)
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `valor_base = (valor * num + off) / den`; `round` acepta resultados no enteros (redondea al mas cercano).
struct Unit {
    suffix: &'static str,
    dim: Dim,
    num: i128,
    den: i128,
    off: i128,
    round: bool,
}

const fn u(suffix: &'static str, dim: Dim, num: i128) -> Unit {
    Unit { suffix, dim, num, den: 1, off: 0, round: false }
}

const UNITS: &[Unit] = &[
    u("Hz", Dim::Frequency, 1),
    u("kHz", Dim::Frequency, 1_000),
    u("MHz", Dim::Frequency, 1_000_000),
    u("GHz", Dim::Frequency, 1_000_000_000),
    u("ns", Dim::Time, 1),
    u("us", Dim::Time, 1_000),
    u("ms", Dim::Time, 1_000_000),
    u("s", Dim::Time, 1_000_000_000),
    u("min", Dim::Time, 60_000_000_000),
    u("h", Dim::Time, 3_600_000_000_000),
    u("B", Dim::Memory, 1),
    u("KB", Dim::Memory, 1_000),
    u("MB", Dim::Memory, 1_000_000),
    u("KiB", Dim::Memory, 1_024),
    u("MiB", Dim::Memory, 1_048_576),
    u("baud", Dim::Baud, 1),
    u("kbaud", Dim::Baud, 1_000),
    u("Mbaud", Dim::Baud, 1_000_000),
    u("bps", Dim::DataRate, 1),
    u("kbps", Dim::DataRate, 1_000),
    u("Mbps", Dim::DataRate, 1_000_000),
    u("uV", Dim::Voltage, 1),
    u("mV", Dim::Voltage, 1_000),
    u("V", Dim::Voltage, 1_000_000),
    u("uA", Dim::Current, 1),
    u("mA", Dim::Current, 1_000),
    u("A", Dim::Current, 1_000_000),
    u("ohm", Dim::Resistance, 1_000),
    u("kohm", Dim::Resistance, 1_000_000),
    u("Mohm", Dim::Resistance, 1_000_000_000),
    u("pF", Dim::Capacitance, 1),
    u("nF", Dim::Capacitance, 1_000),
    u("uF", Dim::Capacitance, 1_000_000),
    u("mF", Dim::Capacitance, 1_000_000_000),
    u("F", Dim::Capacitance, 1_000_000_000_000),
    u("nH", Dim::Inductance, 1),
    u("uH", Dim::Inductance, 1_000),
    u("mH", Dim::Inductance, 1_000_000),
    u("H", Dim::Inductance, 1_000_000_000),
    u("uW", Dim::Power, 1),
    u("mW", Dim::Power, 1_000),
    u("W", Dim::Power, 1_000_000),
    // temperatura: milikelvin. 0 degC = 273,15 K; degF = (F - 32) * 5/9 + 273,15
    Unit { suffix: "K", dim: Dim::Temperature, num: 1_000, den: 1, off: 0, round: false },
    Unit { suffix: "degC", dim: Dim::Temperature, num: 1_000, den: 1, off: 273_150, round: false },
    Unit { suffix: "degF", dim: Dim::Temperature, num: 5_000, den: 9, off: 273_150 * 9 - 160_000, round: true },
    u("rpm", Dim::Rotation, 1),
    // angulo: microgrados. 1 rad = 180/pi grados = 57 295 779,513... udeg (redondeado)
    u("deg", Dim::Angle, 1_000_000),
    Unit { suffix: "rad", dim: Dim::Angle, num: 5_729_577_951_308_232, den: 100_000_000, off: 0, round: true },
    // porcentaje: milesimas de punto porcentual
    u("pct", Dim::Percent, 1_000),
];

/// Busca un sufijo exacto.
pub fn lookup(suffix: &str) -> Option<Dim> {
    UNITS.iter().find(|x| x.suffix == suffix).map(|x| x.dim)
}

/// Todos los sufijos y su magnitud (para la documentacion, el autocompletado y las pruebas).
pub fn all() -> impl Iterator<Item = (&'static str, Dim)> {
    UNITS.iter().map(|x| (x.suffix, x.dim))
}

/// Un literal numerico sin signo: mantisa entera y potencia de diez (`11.0592` = 110592 * 10^-4).
#[derive(Debug, PartialEq, Eq)]
pub struct Decimal {
    pub mantissa: i128,
    pub exp10: i32,
}

impl Decimal {
    /// Lee `123`, `1_000`, `11.0592`, `8e6`, `1.5E-3`. `None` si no es un numero decimal o desborda.
    pub fn parse(text: &str) -> Option<Decimal> {
        let (num, exp) = match text.find(['e', 'E']) {
            Some(i) => (&text[..i], Some(&text[i + 1..])),
            None => (text, None),
        };
        let (int_part, frac_part) = match num.split_once('.') {
            Some((a, b)) => (a, b),
            None => (num, ""),
        };
        let clean = |s: &str| -> Option<String> {
            let s: String = s.chars().filter(|&c| c != '_').collect();
            s.chars().all(|c| c.is_ascii_digit()).then_some(s)
        };
        let (ip, fp) = (clean(int_part)?, clean(frac_part)?);
        if ip.is_empty() {
            return None;
        }
        let mantissa = format!("{ip}{fp}").parse::<i128>().ok()?;
        let mut exp10 = -(fp.len() as i32);
        if let Some(e) = exp {
            let e = e.strip_prefix('+').unwrap_or(e);
            let (neg, digits) = match e.strip_prefix('-') {
                Some(d) => (true, d),
                None => (false, e),
            };
            let digits = clean(digits).filter(|d| !d.is_empty())?;
            let v: i32 = digits.parse().ok().filter(|v| *v <= 60)?;
            exp10 += if neg { -v } else { v };
        }
        Some(Decimal { mantissa, exp10 })
    }
}

/// Maximo comun divisor (siempre positivo).
pub fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Un racional `num/den` normalizado (den > 0, sin factores comunes); `None` si `den` es 0 o desborda.
pub fn ratio(num: i128, den: i128) -> Option<(i128, i128)> {
    if den == 0 {
        return None;
    }
    let g = gcd(num, den).max(1);
    let (n, d) = (num / g, den / g);
    Some(if d < 0 { (n.checked_neg()?, d.checked_neg()?) } else { (n, d) })
}

impl Decimal {
    /// El valor como racional exacto: `11.0592` es 691/62500.
    pub fn as_ratio(&self) -> Option<(i128, i128)> {
        if self.exp10 >= 0 {
            ratio(self.mantissa.checked_mul(10i128.checked_pow(self.exp10 as u32)?)?, 1)
        } else {
            ratio(self.mantissa, 10i128.checked_pow((-self.exp10) as u32)?)
        }
    }
}

/// Por que un literal con unidad no se pudo convertir.
#[derive(Debug, PartialEq, Eq)]
pub enum UnitError {
    /// El valor no es un numero entero de unidades base (`0.5Hz`, `1.5pF`...).
    NotExact(&'static str),
    /// Demasiado grande.
    TooBig,
}

/// Convierte `valor` + `suffix` al entero de la unidad base. `None` si el sufijo no existe.
pub fn to_base(value: &Decimal, suffix: &str) -> Option<Result<(i128, Dim), UnitError>> {
    let unit = UNITS.iter().find(|x| x.suffix == suffix)?;
    Some(convert(value, unit).map(|v| (v, unit.dim)))
}

fn pow10(n: u32) -> Option<i128> {
    10i128.checked_pow(n)
}

fn convert(v: &Decimal, unit: &Unit) -> Result<i128, UnitError> {
    // valor = mantissa * 10^exp10; base = (valor * num + off) / den
    let (p, q) = if v.exp10 >= 0 { (pow10(v.exp10 as u32).ok_or(UnitError::TooBig)?, 1) } else { (1, pow10((-v.exp10) as u32).ok_or(UnitError::TooBig)?) };
    let numer = v.mantissa.checked_mul(unit.num).and_then(|a| a.checked_mul(p)).and_then(|a| unit.off.checked_mul(q).and_then(|o| a.checked_add(o))).ok_or(UnitError::TooBig)?;
    let denom = unit.den.checked_mul(q).ok_or(UnitError::TooBig)?;
    if numer % denom == 0 {
        return Ok(numer / denom);
    }
    if unit.round {
        return Ok((numer + denom / 2) / denom);
    }
    Err(UnitError::NotExact(unit.dim.base_unit()))
}

/// Resultado de leer un literal con unidad.
#[derive(Debug, PartialEq, Eq)]
pub enum Scan {
    Value(i128, Dim),
    NotExact(&'static str),
    TooBig,
}

/// Lee en `src[start..]` un numero decimal pegado a un sufijo de unidad (`8MHz`, `11.0592MHz`, `8e6Hz`, `500ms`). Devuelve el final del
/// literal y su valor, o `None` si ahi no hay un literal con unidad. Con `loose_hz` (BASIC, que no distingue mayusculas) los sufijos de
/// frecuencia se aceptan en cualquier caso (`MHZ`, `khz`).
pub fn scan(src: &str, start: usize, loose: bool) -> Option<(usize, Scan)> {
    let b = src.as_bytes();
    let digits = |j: &mut usize| {
        while *j < b.len() && (b[*j].is_ascii_digit() || b[*j] == b'_') {
            *j += 1;
        }
    };
    let mut j = start;
    digits(&mut j);
    if j == start {
        return None;
    }
    if b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(|c| c.is_ascii_digit()) {
        j += 1;
        digits(&mut j);
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(b.get(k), Some(b'+' | b'-')) {
            k += 1;
        }
        if b.get(k).is_some_and(|c| c.is_ascii_digit()) {
            j = k;
            digits(&mut j);
        }
    }
    let num_end = j;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    let raw = &src[num_end..j];
    let canon = if lookup(raw).is_some() {
        raw
    } else if loose {
        // sin distinguir mayusculas: vale si exactamente una unidad coincide (`MS` es ms, `khz` es kHz, `3.3v` es V)
        let mut found = UNITS.iter().filter(|u| u.suffix.eq_ignore_ascii_case(raw)).map(|u| u.suffix);
        match (found.next(), found.next()) {
            (Some(one), None) => one,
            _ => return None,
        }
    } else {
        return None;
    };
    let dec = Decimal::parse(&src[start..num_end])?;
    Some((
        j,
        match to_base(&dec, canon)? {
            Ok((v, d)) => Scan::Value(v, d),
            Err(UnitError::NotExact(base)) => Scan::NotExact(base),
            Err(UnitError::TooBig) => Scan::TooBig,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str, unit: &str) -> Result<(i128, Dim), UnitError> {
        to_base(&Decimal::parse(text).unwrap(), unit).unwrap()
    }

    #[test]
    fn frequencies_are_exact_in_hz() {
        assert_eq!(v("8", "MHz"), Ok((8_000_000, Dim::Frequency)));
        assert_eq!(v("11.0592", "MHz"), Ok((11_059_200, Dim::Frequency)));
        assert_eq!(v("32.768", "kHz"), Ok((32_768, Dim::Frequency)));
        assert_eq!(v("11_059_200", "Hz"), Ok((11_059_200, Dim::Frequency)));
        assert_eq!(v("8e6", "Hz"), Ok((8_000_000, Dim::Frequency)));
        assert_eq!(v("1.5", "GHz"), Ok((1_500_000_000, Dim::Frequency)));
        assert_eq!(v("0.5", "Hz"), Err(UnitError::NotExact("Hz")));
    }

    #[test]
    fn other_magnitudes_use_their_base_unit() {
        assert_eq!(v("500", "ms"), Ok((500_000_000, Dim::Time)));
        assert_eq!(v("3.3", "V"), Ok((3_300_000, Dim::Voltage)));
        assert_eq!(v("50", "pct"), Ok((50_000, Dim::Percent)));
        assert_eq!(v("4.7", "kohm"), Ok((4_700_000, Dim::Resistance)));
        assert_eq!(v("100", "nF"), Ok((100_000, Dim::Capacitance)));
        assert_eq!(v("1", "KiB"), Ok((1_024, Dim::Memory)));
        assert_eq!(v("9600", "baud"), Ok((9_600, Dim::Baud)));
        assert_eq!(v("25", "degC"), Ok((298_150, Dim::Temperature)));
        assert_eq!(v("300", "K"), Ok((300_000, Dim::Temperature)));
        assert_eq!(v("90", "deg"), Ok((90_000_000, Dim::Angle)));
    }

    #[test]
    fn suffixes_are_case_sensitive() {
        assert_eq!(lookup("MHz"), Some(Dim::Frequency));
        assert_eq!(lookup("mHz"), None);
        assert_eq!(lookup("mH"), Some(Dim::Inductance));
        assert_eq!(lookup("MB"), Some(Dim::Memory));
        assert_eq!(lookup("mV"), Some(Dim::Voltage));
        assert_eq!(lookup("MV"), None);
    }

    #[test]
    fn loose_scan_ignores_case_and_keeps_the_only_ambiguity_exact() {
        let v = |t: &str| match scan(t, 0, true) {
            Some((_, Scan::Value(v, d))) => Some((v, d)),
            _ => None,
        };
        assert_eq!(v("8MHZ"), Some((8_000_000, Dim::Frequency)));
        assert_eq!(v("32.768khz"), Some((32_768, Dim::Frequency)));
        assert_eq!(v("500MS"), Some((500_000_000, Dim::Time)));
        assert_eq!(v("3.3v"), Some((3_300_000, Dim::Voltage)));
        assert_eq!(v("50PCT"), Some((50_000, Dim::Percent)));
        assert_eq!(v("1kib"), Some((1_024, Dim::Memory)));
        // h (hora) y H (henrio) solo se distinguen por la mayuscula: la forma exacta manda y no hay otra lectura
        assert_eq!(v("2h"), Some((7_200_000_000_000, Dim::Time)));
        assert_eq!(v("2H"), Some((2_000_000_000, Dim::Inductance)));
        // en Python (estricto) sigue distinguiendo
        assert!(scan("500MS", 0, false).is_none());
    }

    #[test]
    fn malformed_numbers_are_rejected() {
        assert_eq!(Decimal::parse("1e"), None);
        assert_eq!(Decimal::parse(".5"), None);
        assert_eq!(Decimal::parse("1.2.3"), None);
        assert!(matches!(v("1000000000000000000000000000000", "GHz"), Err(UnitError::TooBig)));
    }
    #[test]
    fn the_xc8_comparison_table_converts_exactly() {
        // 32768UL, 500000UL, 500000UL, 4000000UL, 7372800UL, 11059200UL, 20000000UL
        for (n, u, hz) in [("32.768", "kHz", 32_768), ("500", "kHz", 500_000), ("0.5", "MHz", 500_000), ("4", "MHz", 4_000_000), ("7.3728", "MHz", 7_372_800), ("11.0592", "MHz", 11_059_200), ("20", "MHz", 20_000_000)] {
            assert_eq!(v(n, u), Ok((hz, Dim::Frequency)), "{n}{u}");
        }
        // FCY de 11,0592 MHz con 4 relojes por ciclo
        assert_eq!(11_059_200 / 4, 2_764_800);
    }
}
