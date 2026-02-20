---
title: "BHDL — Board Hardware Description Language"
date: 2025-03-01
summary: "A modern PCB description language written in Rust with flow-based syntax, design intent, power domain scalability, SPICE analysis, and ISO 26262 safety. ~306K lines across 18 crates."
tags: ["hardware", "pcb", "rust", "eda", "electronics"]
ShowToc: true
---

**BHDL** (Board Hardware Description Language) is a hardware description language I'm building in Rust for PCB design. It replaces schematic capture with a text-based flow syntax where connections read like signal paths, design intent is a first-class feature, and the compiler handles everything from component inference to SPICE analysis to safety compliance.

[GitHub](https://github.com/girivs82/bhdl) | ~306K lines of Rust | 18 workspace crates

---

## Why I'm Building This

PCB design in 2025 still revolves around graphical schematic capture tools from the 1990s — KiCad, Altium, OrCAD. They work, but they share fundamental problems:

**Netlists are flat.** A schematic page connects pins with wires. There's no concept of *why* two pins are connected — is this a power distribution path, a signal chain, a feedback network? The intent disappears into a bag of net names. Six months later, you're reverse-engineering your own design.

**No safety net.** Connect a 3.3V output to a 1.8V input? Wire a capacitor backwards? Forget a decoupling cap on a power pin? The schematic editor won't stop you. You discover mistakes during prototype bring-up — or worse, in the field.

**No scalability for repetitive designs.** An FPGA board with 32 power pins, each needing decoupling capacitors, means placing and wiring 64+ components by hand. A DDR3 interface with 8 matched-length byte lanes means clicking through the same routing constraints 8 times. There's no `for` loop in a schematic.

**No IDE tooling.** Schematic editors have no autocomplete, no go-to-definition, no refactoring, no linting. You can't grep a schematic. You can't diff it meaningfully. You can't code-review it.

**Verbosity for common patterns.** A voltage regulator circuit has a known topology — input caps, output caps, feedback divider, compensation network. Every engineer draws it from scratch every time, making the same layout and value choices. There's no abstraction mechanism to capture "buck converter with these specs."

BHDL is my answer: a text-based language where connections flow like signals, intent annotations guide the compiler, power domains scale to hundreds of pins with wildcards, and the toolchain includes everything from SPICE simulation to ISO 26262 safety analysis.

---

## Design Decisions

### Why Flow-Based Syntax?

Schematics are drawn as signal flows — power enters on the left, flows through regulation, and exits on the right. BHDL's `->` syntax mirrors this directly:

```
board PowerRegulator7805 {
    power VIN = 12V @ 1A;
    power VOUT = 5V @ 1A;
    ground GND;

    // Input filtering
    VIN -> input_cap: Cap(100u).+ ;
    input_cap.- -> GND;

    // Voltage regulation
    VIN -> reg: LM7805().IN;
    reg.GND -> GND;
    reg.OUT -> VOUT;

    // Output filtering
    VOUT -> output_cap: Cap(10u).+ ;
    output_cap.- -> GND;

    // LED power indicator
    VOUT -> Res(330).1 -> power_led: LED(green).A;
    power_led.K -> GND;
}
```

You read the circuit top-to-bottom, left-to-right, exactly like you'd trace it on a schematic. Components are instantiated inline where they're connected — `Cap(100u)` creates a 100µF capacitor and `.+` accesses its positive pin. No separate symbol placement step, no separate wiring step.

Bidirectional connections use `<->` for interfaces like I2C and SPI:

```
main_i2c <-> [temp_sensor, eeprom];
```

Flow paths can span multiple components in a single statement. `VOUT -> Res(330).1 -> power_led: LED(green).A` creates a resistor and LED, wires pin 1 of the resistor to VOUT, and wires pin 2 to the LED's anode — all in one line that reads like the signal path.

**Why not netlist syntax?** Because `net VCC_3V3: U1.pin3, U2.pin7, C1.pin1, C2.pin1` tells you nothing about topology. Flow syntax preserves the *path* — what connects to what through what.

### Why Design Intent as a First-Class Feature?

Most EDA tools capture *what* you connected but not *why*. BHDL's `for` keyword attaches intent to any connection:

```
board LM7805_Regulator {
    power VIN = 12V @ 1A for low_noise(max_ripple: 50mV);
    ground GND;

    // Input protection with intent
    net protected_input: VIN -> tvs: TVSDiode(15V).K -> tvs.A -> @GND
        for input_protection(overvoltage: 15V, current_limit: 2A);

    // Filtering with intent
    net filtered_input: @protected_input -> C1: Cap(100n).1 -> C1.2 -> @GND
        for noise_filtering(cutoff: 100kHz, attenuation: 40dB);

    // Output with stability intent
    net regulated_output: reg.VOUT -> C2: Cap(10u).1 -> C2.2 -> @GND
        for low_noise(max_ripple: 10mV);

    // Current-limited indicator
    @VOUT -> R1: Res(330).1 -> R1.2 -> led: LED(green).A -> led.K -> @GND
        for current_limiting(max: 15mA);
}
```

Intent isn't decorative — it's functional. The `noise_filtering(cutoff: 100kHz, attenuation: 40dB)` intent tells the compiler that this RC network should attenuate signals above 100kHz by 40dB. The compiler can verify that the component values actually achieve this. The `current_limiting(max: 15mA)` intent verifies that the resistor value limits LED current to 15mA given the supply voltage.

BHDL ships with 38 intent functions organized across 10 categories:

**Timing:** `delay`, `debounce`, `pulse_stretch`

**Protection:** `input_protection`, `overvoltage_protection`, `esd_protection`

**Signal Processing:** `anti_alias`, `low_noise`, `noise_filtering`

**Analog:** `current_limiting`, `level_shifting`, `voltage_division`, `signal_amplification`

**Digital:** `signal_buffering`

**Measurement:** `precision_measurement`, `control_loop`

**Development:** `debug_only`

**Safety:** `automotive_safety`, `industrial_control`, `medical_safety`

**Power Management:** `power_sequencing`, `voltage_monitoring`, `power_good_signal`, `inrush_limiting`

**Advanced:** `signal_integrity`, `emi_filtering`, `isolation`, `thermal_management`, `voltage_regulation`, `current_sensing`, `communication_interface`, `watchdog_monitoring`, `power_optimization`, `test_point`, `redundancy`, `clock_distribution`, `reset_generation`, `boot_sequencing`

Intent functions are library definitions, not language primitives. You can define your own using the same mechanism the standard library uses.

### Why Power Domain Scalability?

An FPGA development board has hundreds of power pins across a dozen voltage domains. In KiCad, you wire each one manually. In BHDL, power domains scale with wildcards and ranges:

```
board MultiSensorBoard {
    sensor_0: TempSensor();
    sensor_1: TempSensor();
    sensor_2: TempSensor();
    sensor_3: TempSensor();
    fpga: FPGA();

    power_domain @VCC_3V3 = 3.3V @ 5A {
        distribution {
            // Wildcard: expands to sensor_0.VCC, sensor_1.VCC, ...
            sensor[*].VCC;

            // Range: expands to fpga.VCCO[0] through fpga.VCCO[7]
            fpga.VCCO[0..7];

            fpga.VCCAUX;
        }

        decoupling {
            near reg: 10µF @ 1, 1µF @ 2;
            near each fpga.VCCO[0..3]: 100nF @ 1;
            distributed: 100nF @ 10, 10nF @ 20;
        }

        constraints {
            max_voltage_drop: 100mV;
            max_ripple: 50mV;
        }
    }
}
```

`sensor[*].VCC` finds every component instance whose name matches the pattern and connects its VCC pin to the domain. `fpga.VCCO[0..7]` expands to 8 individual pin connections. The `decoupling` block generates capacitors automatically — `near each fpga.VCCO[0..3]: 100nF @ 1` places one 100nF cap near each of the first four VCCO pins. `distributed: 100nF @ 10` scatters 10 additional 100nF caps across the domain.

A realistic FPGA board demonstrates the scale this enables:

```
board FPGADevBoard {
    fpga: GenericFPGA();
    ddr3_0: DDR3_RAM();
    ddr3_1: DDR3_RAM();

    // 32 core power pins, auto-decoupled
    power_domain @VCCINT = 1.0V @ 10A {
        distribution { fpga.VCCINT[0..31]; }
        decoupling {
            near fpga: 220µF @ 2, 100µF @ 4;
            near each pin: 100nF @ 32;
            distributed: 10µF @ 8, 1µF @ 16;
        }
    }

    // 16 DDR3 power pins across 2 chips
    power_domain @VDD_DDR = 1.5V @ 4A {
        distribution {
            ddr3_0.VDD[0..7];
            ddr3_1.VDD[0..7];
        }
        decoupling {
            near ddr3_0: 100µF @ 2, 47µF @ 2, 10µF @ 4;
            near ddr3_1: 100µF @ 2, 47µF @ 2, 10µF @ 4;
            near each pin: 100nF @ 16;
        }
    }

    // Peripherals with wildcards
    power_domain @VCC_3V3 = 3.3V @ 5A {
        distribution {
            ethernet_phy.VCC[0..3];
            usb_hub.VCC[0..1];
            pmod[*].VCC;
            button_pullup[*].1;
        }
    }
}
```

This board has 131 power connections and 200+ decoupling capacitors. In BHDL, it's ~60 lines. In a schematic editor, it's hundreds of manual placements and wires.

### Why a Rich Type System?

BHDL pins have types — `power in`, `signal out`, `ground`, `clock in` — and the compiler checks them:

```
entity DDR3Controller() {
    pin DQ[0..31]: signal inout;
    pin DQS[0..3]_P: signal inout;
    pin A[0..14]: signal out;
    pin CK_P: clock out;
    pin CK_N: clock out;
}

entity PrecisionOpAmp() {
    pin IN_P: signal in;
    pin IN_N: signal in;
    pin OUT: signal out;
    pin V+: power in;
    pin V-: power in;

    attribute input_bias_current = 50pA;
    attribute offset_voltage = 25uV;
    attribute CMRR = 120dB;
}
```

Components are parameterized — `Cap(100nF, type="C0G", package="0603")` — and the compiler uses these parameters for inference and verification. Generic entities accept parameters with constraints:

```
entity BuckController(breakdown_voltage: voltage) {
    pin VIN: power in;
    pin SW: power out;
}

entity VoltageReference(voltage: voltage) {
    pin VIN: power in;
    pin VOUT: signal out;
    pin GND: ground;

    attribute accuracy = 0.05%;
    attribute tempco = 10ppm;
}
```

The `generate` construct creates repeated structures with compile-time iteration:

```
generate for i in 0..7 {
    led_{i}: LED(color: "red");
    led_resistor_{i}: Res(value: 470ohm);
}

generate for i in 0..3 {
    VCC -> Res(10kΩ).1 -> status_led[i]: LED(green).A;
    status_led[i].K -> GND;
}
```

Conditional blocks allow debug-only or configuration-dependent sections:

```
if (debug_mode) {
    VCC_3V3 -> Res(1k).1 -> debug_led: LED(yellow).A;
    debug_led.K -> GND;
}
```

### Why Physical Quantities in the Language?

EDA tools store component values as strings — "100n", "4.7k" — and leave interpretation to the user. BHDL has first-class physical quantities with dimensional analysis:

```
power VCC = 5V @ 2A;
power VCC_3V3 = 3.3V @ 1A;
power VDD_DDR = 1.5V @ 4A;

C1: Cap(100µF, voltage=25V);
R1: Res(10kΩ, tolerance=0.1%, tempco=25ppm);
L1: Inductor(22µH);
```

The unit system supports 15 base units — Volts, Amperes, Ohms, Farads, Henrys, Watts, Hertz, Seconds, Celsius, Kelvin, Percent, Decibels, and more — each with SI prefixes. This gives 60+ distinct unit expressions:

**Resistance:** Ω, mΩ, kΩ, MΩ

**Capacitance:** F, pF, nF, µF, mF

**Inductance:** H, nH, µH, mH

**Voltage:** V, mV, µV, kV

**Current:** A, mA, µA, nA

**Frequency:** Hz, kHz, MHz, GHz

**Time:** s, ms, µs, ns, ps

Unicode is supported natively — `10kΩ` and `100µF` are valid tokens. The compiler evaluates physical expressions at compile time: the intent system can verify that a voltage divider's ratio matches the declared output, or that an RC filter's cutoff frequency matches the `anti_alias(cutoff: 1kHz)` intent.

---

## Compiler Architecture

BHDL uses a 13-pass analysis pipeline. Each pass builds on the previous, transforming source code through progressively richer representations:

```
Source (.bhdl)
    ↓
Pass 1:    Scope Registry — symbol tables, imports, definitions
    ↓
Pass 1.25: Component Instance Registry — track instances for wildcards
    ↓
Pass 1.5:  Power Domain Expansion — expand [*], [0..7], generate decoupling caps
    ↓
Pass 2:    References & Basic Types — validate identifiers, collect diagnostics
    ↓
Pass 2.5:  Monomorphization — specialize generic components
    ↓
Pass 3:    Constant Evaluation — evaluate const expressions, physical quantities
    ↓
Pass 4:    Bounds Checks — verify array bounds, component limits, electrical constraints
    ↓
Pass 5:    Power Analysis — analyze domains, detect voltage conflicts, identify level shifters
    ↓
Pass 6:    Component Inference — infer missing values from circuit context
    ↓
Pass 6.5:  SPICE Synthesis — resolve placeholders using electrical simulation
    ↓
Pass 7:    Power Sequencing — generate startup/shutdown sequences
    ↓
Pass 8:    Attribute Analysis — extract and validate attributes
    ↓
Pass 9:    Flow Tracking & Intent Resolution — track signal flows, resolve intents
    ↓
Pass 10:   Unified Simulation — run mixed-signal simulation
    ↓
Pass 11:   Safety Analysis — ISO 26262 compliance, requirement traceability
    ↓
Backends: Netlist · SPICE · Schematic · BOM · Documentation
```

**Why 13 passes instead of fewer?** Each pass has a single, well-defined responsibility. Power domain expansion (Pass 1.5) must run before reference validation (Pass 2) because wildcard expansion creates new identifiers. Monomorphization (Pass 2.5) must complete before constant evaluation (Pass 3) because generic parameters affect computed values. Component inference (Pass 6) needs type information from all previous passes. SPICE synthesis (Pass 6.5) uses inferred values to run simulation. The passes compose cleanly — each transforms the AST in a way that enables the next.

The fractional numbering (1.25, 1.5, 2.5, 6.5) reflects passes that were inserted between existing ones as the language grew, without renumbering the pipeline.

### Frontend

The parser uses the `rowan` crate for lossless syntax trees — every whitespace character and comment is preserved. This means a formatter can round-trip perfectly: parse → modify → emit produces identical output for unchanged regions. Error recovery is built in: invalid tokens are collected rather than causing a fatal abort, so the parser reports multiple errors per file.

The lexer handles BHDL-specific challenges: disambiguating Unicode unit symbols (`Ω`, `µ`) from identifiers, parsing physical quantities with SI prefixes (`100nF`, `4.7kΩ`), and recognizing flow operators (`->`, `<->`, `|>`) alongside standard punctuation.

---

## Synthesis

The synthesizer in `bhdl-synthesizer` converts the analyzed AST into a netlist. This isn't a simple 1:1 mapping — the synthesizer makes intelligent decisions about component selection, value calculation, and circuit topology.

### Component Inference

When you write `U1: TPS54331(vout=5V)`, the compiler knows the TPS54331 is a buck converter and infers the surrounding circuitry:

```
// This single virtual pin connection:
U1.VOUT -> @VOUT_5V;

// Expands to:
// - Output inductor (calculated ~15µH for 5V output)
// - Bootstrap capacitor (100nF)
// - Output capacitors (2×22µF)
// - Feedback resistor divider (calculated for 5V)
// - Compensation network
// - Soft-start capacitor
```

The knowledge for this expansion comes from component definition files in the standard library, which encode the component's topology, design equations, and recommended values from datasheets.

### Virtual Pin Expansion

Virtual pins (like `VOUT` on a buck converter IC that doesn't have a physical VOUT pin) are synthesized into concrete circuits. The TPS54331's VOUT is actually created by the inductor connected to the SW pin — the synthesizer generates the inductor, output caps, and feedback network to produce the declared output voltage.

### Intent-Aware Generation

Intent annotations influence synthesis decisions. A `low_noise(max_ripple: 10mV)` intent on an output may cause the synthesizer to select ceramic capacitors over electrolytic, add additional high-frequency bypass caps, or specify tighter tolerance components. A `current_limiting(max: 15mA)` intent calculates the exact resistor value given the supply voltage and forward drop.

---

## Schematic Visualization

BHDL includes an interactive schematic viewer that converts netlists to visual schematics:

```
Netlist → Extract → SchematicData (JSON) → ELK.js Layout → Canvas Renderer → HTML
```

The extraction layer (`bhdl-schematic`) converts the internal netlist representation to a serializable schema of components, nets, and connections. The layout engine uses ELK.js (Eclipse Layout Kernel) with a Sugiyama layered algorithm — components are positioned in layers based on signal flow direction, with power rails on the sides and signal paths in the middle.

The Canvas renderer produces standalone HTML files with:

- Component symbols with pin annotations and net labels
- Wire routing with domain-aware preprocessing
- Color coding by net class: signal (blue), power (red), ground (black), differential pairs (green)
- Interactive pan and zoom
- Embedded assets (no external dependencies)

The `bhdl visualize` command generates the schematic directly from source, and `bhdl pipeline` includes it as the final step of a full compilation.

---

## SPICE Analysis

BHDL integrates electrical simulation through the `bhdl-spice` crate, which implements a full SPICE-class solver called GLACIER (General Linear and Analog Circuit Iterative Equation Resolver).

### Core Algorithms

**Modified Nodal Analysis (MNA)** builds the circuit equation matrix. Each component stamps its contribution into the matrix — resistors add conductance terms, capacitors add time-dependent terms, voltage sources add constraint equations. The matrix is sparse (most entries are zero) and solved via LU decomposition.

**Newton-Raphson iteration** handles nonlinear components (diodes, MOSFETs, op-amps). At each iteration, nonlinear elements are linearized around the current operating point, the linearized system is solved, and the operating point is updated. Convergence is checked against voltage and current tolerances.

**Transient analysis** uses backward Euler integration to step through time. An adaptive timestep controller (PID-based) adjusts step size based on convergence speed — smaller steps near fast transients, larger steps during steady state.

### Analysis Modes

- **DC operating point** — find the steady-state voltages and currents
- **AC small-signal** — frequency response via perturbation analysis
- **Transient** — time-domain simulation with adaptive stepping
- **Component role detection** — automatically classify components by their circuit function

### Intent-Driven Strategy

The SPICE engine selects simulation strategies based on design intent. A `current_sharing` intent triggers symmetry-aware solving. A `precision_measurement` intent increases convergence tolerances. The engine topology analyzer identifies circuit patterns (voltage dividers, feedback loops, power stages) and optimizes the solver accordingly.

---

## Simulation and Testbenches

The simulation framework (`bhdl-sim`, `bhdl-testbench`) provides behavioral simulation with waveform capture:

### Stimulus Generation

Testbenches define input stimuli — pulse, sine, ramp, chirp waveforms — and reference signals by net name (`@VCC`), pin (`U1.FB`), or computed expression (`R1.power`).

### Waveform Capture

Three output formats:

- **VCD** (Value Change Dump) — standard format compatible with GTKWave and other waveform viewers
- **CSV** — comma-separated samples for spreadsheet analysis
- **JSON** — structured data with metadata for programmatic processing

### Measurements and Assertions

Built-in measurement functions extract overshoot, settling time, rise/fall time, and ripple from simulation results. Assertions specify temporal constraints and range violations — the simulation reports pass/fail with the exact time and signal values at the point of violation.

### Mixed-Signal Modes

The simulator supports four resolution modes: digital timing (nanosecond), analog (microsecond), mixed (auto-region switching), and RF (gigahertz). The engine automatically selects the appropriate mode based on circuit content, or you can specify it via intent.

### Fault Injection

The fault injection engine introduces failures — overcurrent, overvoltage, open circuit, parametric drift — into the simulation to test circuit robustness. This feeds directly into the safety analysis pipeline.

---

## Safety: ISO 26262

The `bhdl-safety` crate implements ISO 26262 functional safety analysis for automotive and safety-critical designs.

### ASIL Annotations

Components and circuits can be annotated with Automotive Safety Integrity Level requirements — ASIL A through ASIL D, plus QM (Quality Management) for non-safety items. The safety analysis tracks requirements through the design hierarchy:

```
SafetyRequirement {
    id: "SR-001",
    asil_level: ASIL_D,
    satisfaction_method: "hardware redundancy",
    coverage: 99.1%
}
```

### FMEA (Failure Mode and Effects Analysis)

The compiler performs automated FMEA by analyzing each component for potential failure modes:

**Failure types:** Stuck, Short Circuit, Open Circuit, Parametric Drift

**Failure effects:** Loss of function, Degraded performance, Propagation to other subsystems, Latent failures

For each failure mode, the analysis computes a Risk Priority Number (RPN), Single Point Fault Metric (SPFM), and Diagnostic Coverage (DC). The output is a complete FMEA table with component, failure mode, effect, and mitigation for every entry.

### Redundancy Analysis

The redundancy analyzer identifies and evaluates redundant circuit paths:

**Redundancy types:** 1oo1 (single channel), 1oo2 (dual redundant), 2oo3 (triple modular redundancy with voting)

**Common Cause Failure (CCF)** analysis identifies components that share failure causes — same power domain, same component type, physical proximity — and applies beta factors to adjust failure rate calculations. The analysis computes effective failure rates accounting for both independent and correlated failures.

### Requirement Traceability

The safety system maintains a traceability matrix from top-level safety requirements down to individual circuit elements. Hierarchical requirement decomposition tracks which components satisfy which requirements, with coverage percentage calculations and circular dependency detection.

---

## Standard Library

The standard library (`bhdl-stdlib`) provides 137+ component definitions across 80 BHDL files, plus the 38 intent functions described earlier.

### Components

**Passives:** Resistor, Capacitor, Inductor, LED, Diode, Fuse, TVS Diode — each parameterized with value, tolerance, package, voltage/power rating

**Active Devices:** MOSFET, BJT, Op-Amp — with behavioral models for simulation

**Voltage Regulators:** LM7805 (linear), LM317 (adjustable linear), TPS54331 (3A buck), TPS54302 (2A buck), LM2596 (3A buck) — each with virtual pin expansion for automatic surrounding circuit generation

**Power Protection:** SS34 Schottky diode, TVS diodes, fuses

**MCU:** STM32F103C8T6 with full pinout

**Connectors:** Test points, PMOD, JTAG, standard headers

### Component Database

The `bhdl-components` crate provides a component database with KiCad symbol import and supplier API integration. You can import existing KiCad libraries and use them directly in BHDL designs, preserving footprint assignments and supplier part numbers.

### Templates

Higher-level templates encode common circuit patterns — buck converter, linear regulator, LED driver — as parameterized building blocks. A buck converter template encodes the topology, design equations (inductor value from switching frequency and ripple current), and recommended component selections from the datasheet. You specify the input voltage, output voltage, and current — the template generates the complete circuit.

---

## IDE Tooling

### Language Server (LSP)

The `bhdl-lsp` crate implements a Language Server Protocol server with 22 features:

1. **Diagnostics** — real-time parse and semantic errors as you type
2. **Completion** — intent functions, keywords, component names, pin names
3. **Hover** — documentation for symbols, components, intent functions
4. **Go to Definition** — jump to component or entity definition
5. **Find References** — find all uses of a symbol across files
6. **Rename** — safe symbol renaming with cross-file updates
7. **Document Symbols** — outline view (boards, entities, nets, components)
8. **Workspace Symbols** — cross-file symbol search
9. **Semantic Tokens** — syntax highlighting for keywords, types, variables, functions
10. **Signature Help** — parameter hints for intent functions and components
11. **Code Actions** — quick fixes and refactorings
12. **Inlay Hints** — inline type and value annotations
13. **Folding Ranges** — code folding for boards, entities, power domains
14. **Call Hierarchy** — function call tree (prepare, incoming, outgoing)
15. **Selection Range** — smart selection expansion
16. **Document Highlight** — highlight all occurrences of a symbol
17. **Code Lens** — reference counts, test indicators
18. **Document Link** — clickable import paths
19. **Formatting** — document and range formatting
20. **On-Type Formatting** — auto-format as you type
21. **Commands** — custom LSP commands for BHDL-specific operations
22. **Range Diagnostics** — diagnostic ranges with severity levels

### CLI

The `bhdl` command-line tool provides 9 commands:

| Command | Description |
|---------|-------------|
| `bhdl parse <file>` | Parse and check syntax (output: AST, pretty-print, JSON) |
| `bhdl analyze <file>` | Run semantic analysis with diagnostics |
| `bhdl synthesize <file>` | Generate netlist (JSON or SPICE format) |
| `bhdl visualize <file>` | Generate interactive schematic (HTML) |
| `bhdl spice <file>` | Run SPICE analysis (DC, AC, transient) |
| `bhdl pipeline <file>` | Complete flow: parse → analyze → synthesize → visualize |
| `bhdl simulate <file>` | Run testbench with waveform capture (VCD/CSV/JSON) |
| `bhdl intents <file>` | Analyze design intents, show synthesis hints |
| `bhdl doc <file>` | Generate documentation (BOM, power budget, power tree) |

Each command supports `--format` for output selection and `--verbose` for detailed diagnostics.

---

## What Makes This Different

Most PCB tools treat schematic capture, simulation, BOM generation, and safety analysis as separate workflows with separate tools. BHDL integrates them into a single compilation model.

**KiCad** is the standard open-source option — excellent schematic and layout editors, but everything is graphical. No text-based abstraction, no loops, no intent, no simulation (without external SPICE), no safety analysis. Repetitive designs are tedious.

**Altium Designer** is the professional standard with integrated simulation and constraint management, but it's expensive ($10K+ per seat), Windows-only, and still fundamentally graphical. Power-aware design requires manual rule setup.

**SKiDL** embeds PCB description in Python. This gives you Python's abstraction (loops, functions, classes) but no domain-specific features — no intent system, no power domain scalability, no integrated SPICE, no safety analysis. It generates KiCad netlists.

**Chisel (for PCBs)** doesn't exist — Chisel is an FPGA HDL. There's no equivalent text-based PCB tool with a rich type system, simulation, and safety analysis in a single language.

**BHDL's difference** is that everything lives in one compilation model:

- The **flow syntax** preserves circuit topology — you can read the signal path, not just the net list
- **Intent** is preserved through compilation, so the synthesizer and analyzer can verify that the circuit actually achieves its goals
- **Power domain scalability** handles hundreds of pins with wildcards and ranges, generating decoupling automatically
- **SPICE analysis** runs inside the compiler, using Newton-Raphson and MNA on the same circuit representation
- **Safety analysis** produces FMEA, redundancy analysis, and requirement traceability from the design, not a separate spreadsheet
- The **standard library** provides components with virtual pin expansion — a buck converter IC expands into the complete surrounding circuit
- **IDE tooling** (22 LSP features, 9 CLI commands) makes text-based PCB design practical for daily work

| | BHDL | KiCad | Altium | SKiDL |
|---|---|---|---|---|
| Syntax | Flow-based (`->`) | Graphical | Graphical | Python DSL |
| Design intent | First-class (`for`) | None | None | None |
| Power domains | Wildcards, ranges, auto-decoupling | Manual | Rules-based | Manual |
| Type system | Typed pins, physical quantities | None | Basic | Python types |
| SPICE analysis | Integrated (Newton-Raphson, MNA) | External | Integrated | External |
| Safety (ISO 26262) | FMEA, ASIL, redundancy | None | None | None |
| Component inference | Virtual pins, value calculation | None | None | None |
| Generate / loops | `generate for i in 0..N` | None | None | Python loops |
| IDE tooling | LSP (22 features) | Schematic editor | Schematic editor | Python IDE |
| Output | Netlist, SPICE, schematic, BOM | Netlist, Gerber | Netlist, Gerber | KiCad netlist |

---

## Project Structure

```
crates/
  bhdl-parser/       Lexer and parser (rowan lossless syntax trees)
  bhdl-ast/          Abstract syntax tree with semantic analysis
  bhdl-analyzer/     13-pass analysis engine (core compiler pipeline)
  bhdl-synthesizer/  AST → netlist with component inference
  bhdl-netlist/      Core netlist types, nodes, instances, pins
  bhdl-cli/          Command-line interface (9 commands)
  bhdl-lsp/          Language server (22 IDE features)
  bhdl-components/   Component database, KiCad import, supplier APIs
  bhdl-common/       Shared types: IntentRegistry, AnalysisData
  bhdl-spice/        GLACIER solver: Newton-Raphson, MNA, transient
  bhdl-stdlib/       Standard library (137+ components, 38 intents)
  bhdl-safety/       ISO 26262: ASIL, FMEA, redundancy analysis
  bhdl-sim/          Behavioral simulation engine (mixed-signal)
  bhdl-testbench/    Testbench framework, waveform capture (VCD/CSV/JSON)
  bhdl-simulation/   Advanced simulation with fault injection
  bhdl-layout/       Physical layout and PCB routing assistance
  bhdl-schematic/    Schematic visualization (ELK.js + Canvas)
  bhdl-core/         Core shared infrastructure

examples/
  7805_regulator_v2.bhdl     Linear regulator with filtering
  simple_regulator.bhdl      Minimal regulator example
  tests/circuits/realistic/  60+ test circuits:
    buck_converter_*.bhdl    Buck converters (TPS54331, LM2596, TPS54302)
    fpga_dev_board_*.bhdl    FPGA board with 131 power connections
    ddr3_routing_example.bhdl DDR3 with matched-length routing
    precision_opamp_*.bhdl   Precision analog with routing constraints
    mixed_signal_*.bhdl      Mixed-signal with intents
    555_astable_oscillator.bhdl  Classic 555 timer circuit
```

---

## Current Status

The compiler pipeline from source through all 13 analysis passes is functional. The parser handles the full v2.0 grammar with flow-based syntax, power domains, generate constructs, intents, and physical quantities. The analyzer runs scope resolution, power domain expansion with wildcards and ranges, monomorphization, constant evaluation, component inference, SPICE synthesis, and safety analysis.

The synthesizer generates netlists with virtual pin expansion and intent-aware component selection. The SPICE engine (GLACIER) runs DC, AC, and transient analysis with Newton-Raphson convergence. The schematic visualizer produces interactive HTML schematics via ELK.js layout.

The standard library covers common passives, active devices, voltage regulators (linear and switching), and 38 intent functions. The LSP server provides 22 IDE features including completion, hover, go-to-definition, and real-time diagnostics. The CLI exposes 9 commands for the complete workflow from parsing through visualization and documentation generation.
