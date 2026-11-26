"use strict";

const fs = require("node:fs");
const path = require("node:path");

function projectTools(file, workspace, configured, devicesDir) {
  const root = path.resolve(workspace);
  const candidate = path.resolve(file);
  // Inicio de PyPIC puede llamar esta función sin un editor activo y pasar la
  // carpeta del proyecto. No debemos subir al padre en ese caso: allí no están
  // Cargo.toml ni los Device Packs.
  let dir = fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()
    ? candidate
    : path.dirname(candidate);
  while (dir === root || (!path.relative(root, dir).startsWith("..") && !path.isAbsolute(path.relative(root, dir)))) {
    const devices = path.join(dir, "devices");
    if (fs.existsSync(path.join(dir, "Cargo.toml")) && fs.existsSync(devices)) {
      const name = process.platform === "win32" ? "pypic.exe" : "pypic";
      const debug = path.join(dir, "target", "debug", name);
      const release = path.join(dir, "target", "release", name);
      const executable = configured === "pypic" ? [debug, release].find((candidate) => fs.existsSync(candidate)) || configured : configured;
      return { executable, devices: devicesDir || devices };
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return { executable: configured, devices: devicesDir };
}

const info = {
  uint8: "Entero sin signo de 8 bits: 0 a 255; ocupa 1 byte de RAM cuando es variable.",
  uint16: "Entero sin signo de 16 bits: 0 a 65535; ocupa 2 bytes de RAM cuando es variable y se emula en PIC16F887.",
  uint32: "Entero sin signo de 32 bits. Puede utilizarse como constante; consulta los límites del backend para variables.",
  int8: "Entero con signo de 8 bits: -128 a 127.",
  int16: "Entero con signo de 16 bits: -32768 a 32767.",
  Const: "Constante resuelta en compilación, por ejemplo `LIMITE: Const[uint8] = 10`.",
  CLOCK_HZ: "Forma manual de declarar la frecuencia del sistema en Hz para los cálculos de tiempo: no configura el oscilador. Prefiere `CLOCK: Internal[4 * MHz]`.",
  CLOCK: "Declara la fuente y frecuencia de reloj en compilación: `CLOCK: Internal[4 * MHz]`, `Crystal[20 * MHz]` o `ExternalClock[20 * MHz]`. Con un oscilador externo, RA7 (OSC1/CLKIN) y RA6 (OSC2/CLKOUT) dejan de estar disponibles como E/S. No combinar con CLOCK_HZ.",
  Internal: "Oscilador interno del PIC16F887: frecuencias nominales de 31 kHz a 8 MHz, en pasos compatibles con OSCCON.",
  Crystal: "Cristal o resonador en OSC1/OSC2; PyPIC selecciona LP, XT o HS según la frecuencia y reserva RA7 y RA6.",
  ExternalClock: "Señal de reloj externa en CLKIN/RA7: modo EC; admite hasta 20 MHz en PIC16F887.",
  MHz: "Unidad de frecuencia para CLOCK: un MHz equivale a 1 000 000 Hz, resuelto en compilación.",
  kHz: "Unidad de frecuencia para CLOCK: un kHz equivale a 1000 Hz, resuelto en compilación.",
  Hz: "Unidad de frecuencia para CLOCK en hercios, resuelta en compilación.",
  PIC16F887: "Dispositivo de destino: se declara con `from pypic.device import PIC16F887`.",
  ADC: "ADC de 10 bits. `ADC.start()` configura el periférico y `ADC.read(AN0)` lee un canal.",
  AN0: "Canal analógico AN0, asociado a RA0 en PIC16F887.",
  PWM1: "PWM del módulo CCP1 (pin RC2). `PWM1.start(frequency=1000)` fija el periodo exacto; `PWM1.duty(v)` toma cuentas de 10 bits (0 a 1023), no un porcentaje.",
  PWM2: "PWM del módulo CCP2 (pin RC1). Comparte Timer2 con PWM1: un solo periodo por programa.",
  duty: "`PWM1.duty(v)` escribe el ciclo de trabajo en cuentas de TOSC x prescaler (10 bits); el periodo completo tiene 4 x (PR2 + 1) cuentas.",
  EEPROM: "EEPROM de datos de 256 bytes. `EEPROM.read(a)` lee un byte; `EEPROM.write(a, v)` escribe y espera hasta 6 ms; `EEPROM.start_write(a, v)` no espera.",
  UART: "EUSART asíncrono: `UART.start(baud=9600)` configura transmisión y recepción.",
  pause: "`pause(ms)` espera una cantidad de milisegundos; consulta el costo y los límites de `pypic.system.time`.",
  read: "Lectura del periférico. `ADC.read(AN0)` devuelve uint16; `UART.read()` recibe un byte y espera si es necesario.",
  start: "Configura e inicia el periférico. Los parámetros dependen de si se usa ADC, UART o PWM.",
  write_text: "`UART.write_text(texto)` transmite texto ASCII y f-strings soportados sin reservar un búfer de texto completo en RAM.",
  sending: "`UART.sending()` indica si el registro de desplazamiento de transmisión sigue ocupado.",
};

/** Ayuda al pasar el cursor sobre una palabra de PyPIC BASIC (sin distinguir mayusculas). */
const basicInfo = {
  device: "`DEVICE PIC16F887`: el PIC para el que esta escrito el programa. Obligatorio.",
  clock: "`CLOCK INTERNAL 4 MHZ`, `CLOCK CRYSTAL 20 MHZ` o `CLOCK EXTERNAL 20 MHZ`: la fuente y la frecuencia del reloj. Con un oscilador externo RA6 y RA7 son del oscilador.",
  symbol: "`SYMBOL k = 5` (constante), `SYMBOL x = b3` (otro nombre para una variable) o `SYMBOL led = D.0` (un pin).",
  dim: "`DIM x AS BYTE|WORD|BIT` y `DIM t(16) AS BYTE` (arreglo). Las variables `b0`..`b27` (BYTE) y `w0`..`w13` (WORD) existen sin declarar y no se solapan.",
  high: "`HIGH pin`: pone el pin en alto. El pin pasa a ser salida al empezar el programa.",
  low: "`LOW pin`: pone el pin en bajo.",
  toggle: "`TOGGLE pin`: invierte el pin.",
  pause: "`PAUSE ms`: espera exacta al ciclo, con una constante o una variable.",
  pauseus: "`PAUSEUS us`: espera en microsegundos (una constante).",
  readadc: "`READADC canal, b0`: lee el canal ANn y guarda los 8 bits de arriba de los 10.",
  readadc10: "`READADC10 canal, w3`: lee los 10 bits en una variable WORD.",
  read: "`READ direccion, b0`: lee un byte de la EEPROM de datos.",
  write: "`WRITE direccion, valor`: escribe un byte en la EEPROM de datos (espera hasta 6 ms).",
  pwmout: "`PWMOUT C.2, periodo, ciclo`: PWM1 en C.2 o PWM2 en C.1; el periodo es PR2 (0 a 255) y el ciclo de trabajo tiene 10 bits.",
  sertxd: "`SERTXD(\"x = \", #b0, CR, LF)`: envia por la UART (9600 8N1): textos, #valor en decimal, CR, LF y bytes.",
  serrxd: "`SERRXD(b0)`: espera un byte por la UART y lo guarda en una variable BYTE.",
  gosub: "`GOSUB etiqueta`: llama a una subrutina (la etiqueta termina en `RETURN`); cuenta en la pila de hardware.",
  interrupt: "`ON INTERRUPT etiqueta`: la etiqueta (que termina en RETURN) es la rutina de interrupcion; `ENABLE INTERRUPTS` sube GIE y la fuente se habilita aparte (TMR2.interrupt_on, INTCON.T0IE.set...).",
  asm: "`ASM nombre ... END ASM`: una rutina en ensamblador PIC-AS, que se llama con `GOSUB nombre`; usa las variables por su nombre y termina en RETURN.",
  enable: "`ENABLE INTERRUPTS`: sube GIE (`DISABLE INTERRUPTS` lo baja).",
  select: "`SELECT CASE variable ... CASE 1, 2 ... CASE 5 TO 9 ... CASE ELSE ... END SELECT`.",
};

const members = {
  ADC: ["start", "select", "go", "done", "result", "read"],
  EEPROM: ["read", "write", "start_write", "busy", "failed"],
  PWM1: ["start", "duty", "stop"],
  PWM2: ["start", "duty", "stop"],
  UART: ["start", "write", "read", "send", "recv", "tx_ready", "rx_ready", "sending", "overrun", "framing_error", "clear_overrun", "write_text", "write9", "send9", "read9", "recv9"],
};

/** Palabras que se sugieren en un programa BASIC; despues de `OBJETO.` van los metodos del SDK. */
const basicWords = ["DEVICE", "CLOCK", "SYMBOL", "DIM", "AS", "BYTE", "WORD", "BIT", "LET", "IF", "THEN", "ELSE", "ELSEIF", "ENDIF", "FOR", "TO", "STEP", "NEXT", "DO", "LOOP", "WHILE", "WEND", "UNTIL", "SELECT CASE", "CASE", "END SELECT", "GOTO", "GOSUB", "RETURN", "EXIT", "END", "HIGH", "LOW", "TOGGLE", "OUTPUT", "INPUT", "PAUSE", "PAUSEUS", "READADC", "READADC10", "READ", "WRITE", "PWMOUT", "SERTXD", "SERRXD", "ON INTERRUPT", "ENABLE INTERRUPTS", "DISABLE INTERRUPTS", "ASM", "END ASM", "AND", "OR", "XOR", "NOT", "MOD", "INTERNAL", "CRYSTAL", "EXTERNAL", "MHZ", "KHZ", "HZ", "CR", "LF", "TAB"];

function basicCompletions(prefix) {
  const match = /\b(ADC|UART|EEPROM|PWM[12])\.\w*$/i.exec(prefix);
  if (match) return members[match[1].toUpperCase()];
  if (/\.\w*$/.test(prefix)) return [];
  return basicWords;
}

function completions(prefix) {
  const match = /\b(ADC|UART|EEPROM|PWM[12])\.\w*$/.exec(prefix);
  if (match) return members[match[1]];
  if (/\.\w*$/.test(prefix)) return [];
  return ["uint8", "uint16", "uint32", "int8", "int16", "int32", "Const", "CLOCK", "Internal", "Crystal", "ExternalClock", "MHz", "kHz", "Hz", "CLOCK_HZ"];
}

function isPyPic(text) {
  return /^\s*from\s+pypic(?:\.[A-Za-z_]\w*)+\s+import\s+/m.test(text);
}

function declaredDevice(text) {
  // Python: from pypic.device import PIC16F887; BASIC: DEVICE PIC16F887
  const match = /^\s*from\s+pypic\.device\s+import\s+(PIC\d+[A-Z]\d+)\s*$/m.exec(text) || /^\s*DEVICE\s+(PIC\d+[A-Z]\d+)\s*(?:'.*)?$/im.exec(text);
  return match ? match[1].toUpperCase() : undefined;
}

function diagnostics(text, file) {
  const prefix = `${file}:`;
  return text.split(/\r?\n/).filter((line) => line.startsWith(prefix)).flatMap((line) => {
    const match = /^(\d+):(\d+): (error|warning)\[([^\]]+)\]: (.+)$/.exec(line.slice(prefix.length));
    return match ? [{ line: Number(match[1]) - 1, column: Number(match[2]) - 1, severity: match[3], code: match[4], message: match[5] }] : [];
  });
}

module.exports = { info, basicInfo, isPyPic, diagnostics, completions, basicCompletions, projectTools, declaredDevice };
