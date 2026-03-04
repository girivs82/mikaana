---
title: "Tutorials"
summary: "Learn to design hardware with skalp — choose a language track to get started."
---

## Choose Your Track

skalp compiles both its own language and VHDL. Pick the tutorial that matches your background:

---

### [skalp Language Tutorial](skalp/)

**Build a complete UART peripheral from scratch.**

10 chapters that take you from your first entity to clock domain crossings, safety annotations, and Rust-based testbenches. Best if you want to learn the skalp language itself.

1. [Getting Started](skalp/01-getting-started/) — Entities, signals, `on(clk.rise)`
2. [State Machines](skalp/02-state-machines/) — UART transmitter with FSM and baud timing
3. [UART Receiver](skalp/03-uart-receiver/) — Mid-bit sampling and edge detection
4. [Arrays and Generics](skalp/04-arrays-and-generics/) — Parameterized FIFO buffering
5. [Parameterization](skalp/05-parameterization/) — Const generics and configurable designs
6. [Structs and Composition](skalp/06-structs-and-composition/) — Hierarchical design with struct ports
7. [Enums and Matching](skalp/07-enums-and-matching/) — Type-safe FSMs with exhaustive matching
8. [Clock Domain Crossing](skalp/08-clock-domain-crossing/) — CDC safety with clock lifetimes
9. [Safety and Annotations](skalp/09-safety-and-annotations/) — TMR, trace, breakpoints
10. [Testing](skalp/10-testing/) — Async Rust testbenches with full coverage

---

### [VHDL with skalp Tutorial](vhdl/)

**Use your existing VHDL designs with skalp's compiler, simulator, and Rust test framework.**

9 chapters that walk through progressively complex VHDL designs — counters, FSMs, generics, hierarchical systems — compiled and tested with skalp. Best if you already know VHDL and want to use skalp as your build and verification tool.

1. [Getting Started](vhdl/01-getting-started/) — Compile and simulate `counter.vhd`
2. [Combinational Logic](vhdl/02-combinational-logic/) — Multiplexers, `process(all)`, `case/when`
3. [Clocked Processes and FSMs](vhdl/03-processes-and-fsms/) — Timers, I2C controller, enumerations
4. [Generics, Records, and Arrays](vhdl/04-generics-and-types/) — GPIO controller, edge detection
5. [Hierarchical Design](vhdl/05-hierarchical-design/) — Multi-entity systems, port maps
6. [Testing VHDL with Rust](vhdl/06-testing-with-rust/) — `Testbench` API, waveforms, coverage
7. [skalp Integration](vhdl/07-skalp-integration/) — Pragmas, formal verification, mixed designs
8. [VHDL-2019 Features](vhdl/08-vhdl-2019/) — Interfaces, views, generic types
9. [Real-World Project](vhdl/09-real-world-project/) — SPI master capstone with full test suite
