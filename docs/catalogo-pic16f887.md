# Catálogo del PIC16F887: periféricos, protocolos y módulos del sistema

Catálogo aportado por el usuario (2026-11-23) como base para `peripherals.json` del Device Pack (#23) y para decidir cómo se importa cada cosa.
**Contraste con DS40001291H:** los totales y características marcados con ✔ coinciden con las páginas 1–2 del datasheet; lo marcado con ◦ no está en esas páginas
y debe verificarse en el capítulo correspondiente antes de codificarlo en el pack (regla de `AGENTS.md`).

## 1. Periféricos integrados

| Periférico | Cantidad | Características | Contraste |
|---|---|---|---|
| GPIO | 36 | 35 E/S y 1 solo entrada (RE3) | ✔ (el pack ya lo modela) |
| Timer0 | 1 | Temporizador de 8 bits | ✔ |
| Timer1 | 1 | Temporizador de 16 bits | ✔ |
| Timer2 | 1 | Temporizador de 8 bits | ✔ |
| ADC | 1 | 10 bits, 14 canales analógicos | ✔ |
| Comparadores | 2 | Comparación de voltajes analógicos | ✔ |
| CCP | 1 | Captura, comparación y PWM | ✔ |
| ECCP | 1 | PWM mejorado, control de puente H | ✔ |
| MSSP | 1 | Comunicación SPI o I²C | ✔ |
| EUSART | 1 | Comunicación serial síncrona y asíncrona | ✔ |
| CVREF | 1 | Referencia de voltaje programable | ✔ |
| SR Latch | 1 | Biestable Set/Reset | ✔ (p.1: «SR Latch mode») |
| WDT | 1 | Watchdog Timer | ✔ |
| IOC | 8 pines | Interrupción por cambio en PORTB | ✔ |
| INT | 1 | Interrupción externa RB0 | ◦ |
| ULPWU | 1 | Despertar de consumo ultrabajo | ✔ |

## 2. Protocolos de comunicación

| Protocolo | Soporte | Cómo |
|---|---|---|
| UART / USART | Sí | EUSART por hardware |
| SPI Master / Slave | Sí | MSSP por hardware |
| I²C Master / Slave | Sí | MSSP por hardware |
| LIN | Compatible | Mediante EUSART (✔ p.1: «LIN 2.0») |
| RS-232 | Requiere transceptor externo | Niveles de tensión fuera del PIC |
| RS-485 | Requiere transceptor externo | ✔ p.1 |
| 1-Wire | Mediante software | ◦ no es un periférico del chip |
| CAN, USB, I²S | **No** | Sin soporte de hardware |

## 3. Puertos GPIO

| Puerto | Pines | Cantidad |
|---|---|---|
| PORTA | RA0–RA7 | 8 |
| PORTB | RB0–RB7 | 8 |
| PORTC | RC0–RC7 | 8 |
| PORTD | RD0–RD7 | 8 |
| PORTE | RE0–RE3 | 4 |
| **Total** | | **36** |

RE3 funciona únicamente como entrada cuando se deshabilita MCLR (✔, coincide con el pack: `input_only`).

## 4. Administración del sistema

| Módulo | Función | Contraste |
|---|---|---|
| POR | Reset al encender | ✔ |
| BOR | Reset por bajo voltaje | ✔ |
| PWRT | Temporizador de encendido | ✔ |
| OST | Estabilización del oscilador | ✔ |
| FSCM | Monitor de fallos del reloj | ✔ (p.1: «Crystal fail detect», FCMEN) |
| INTOSC | Oscilador interno configurable | ✔ |
| Sleep | Modo de bajo consumo | ✔ |
| ICSP | Programación en circuito | ✔ |
| ICD | Depuración en circuito | ✔ («on board») |

## 5. Características

| Característica | Especificación | Contraste |
|---|---|---|
| Memoria de programa | Flash, 8192 instrucciones de 14 bits (14 KB) | ✔ |
| RAM | 368 B | ✔ |
| EEPROM | 256 B | ✔ |
| Frecuencia máxima | 20 MHz (5 MIPS) | ✔ (200 ns por ciclo) |
| Arquitectura | Harvard, RISC | ◦ |
| CPU | 8 bits | ✔ |
| Instrucciones | 35 | ✔ |
| Oscilador interno | «32 kHz–8 MHz» | ◦ el datasheet dice 8 MHz a **31 kHz** (nominal) |
| Pila de hardware | 8 niveles | ✔ |
| Alimentación | 2–5,5 V | ✔ |
| Encapsulado | 40 o 44 pines | ✔ (PDIP-40, QFN/TQFP-44) |

## 6. Librerías propuestas para el SDK (provisional)

Propuesta recibida. **Decidido (2026-11-23):** espacio `pyxpic.*` (no `machine`), hardware y software en módulos distintos, y subpaquetes por categoría (`pyxpic.periph.*`, `pyxpic.system.*`, `pyxpic.soft.*`); ver `docs/sdk-modulos.md`:

| Hardware / sistema | Software (sobre cualquier GPIO) |
|---|---|
| gpio, adc, timer, pwm, ccp, eccp, uart, spi, i2c, comparator, vref, eeprom, flash, irq, wdt, clock, power, latch | softspi, softi2c, softuart |
