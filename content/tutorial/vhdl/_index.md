---
title: "VHDL with skalp Tutorial"
summary: "Compile, simulate, and test your VHDL designs with skalp — no ModelSim, no license servers, just cargo test."
---

## VHDL Designs, Modern Tooling

If you already write VHDL, you know the pain: expensive simulators, slow compile-edit-test cycles, and testbenches that are harder to write than the design itself. skalp changes the workflow while keeping your VHDL code exactly as it is.

This tutorial walks through progressively complex VHDL designs — each one compiled with `skalp build`, simulated with `skalp sim`, and tested with Rust-based testbenches using `cargo test`. By the end, you will have:

- **Compiled** counters, multiplexers, FSMs, and bus systems with skalp's VHDL frontend
- **Tested** every design with async Rust testbenches — no SystemVerilog testbench boilerplate
- **Used** skalp-specific features: formal verification pragmas, waveform dumps, and coverage
- **Exercised** VHDL-2019 features (interfaces, views) that most free tools do not support
- **Built** a complete SPI master with generics, generate statements, and a full test suite

---

## Prerequisites

This tutorial assumes you already know VHDL. You should be comfortable with:

- Entities, architectures, and port declarations
- `process` blocks, `rising_edge()`, and signal assignment
- `std_logic`, `std_logic_vector`, `unsigned`, `signed`
- Basic FSM patterns with `case` statements

You do **not** need to know Rust, though some familiarity helps from Chapter 6 onward. The tutorial explains Rust concepts where they appear.

---

## Installation

Install skalp from source:

```bash
git clone https://github.com/girivs82/skalp.git
cd skalp
cargo build --release
```

Add the binary to your PATH:

```bash
export PATH="$PATH:$(pwd)/target/release"
```

Verify:

```bash
skalp --version
```

Create a project for the tutorial:

```bash
skalp new vhdl-tutorial
cd vhdl-tutorial
```

This creates a project with `skalp.toml` and a `src/` directory. Each chapter adds VHDL files to this project.

---

## Chapters

1. **[Getting Started](01-getting-started/)** — Compile and simulate a VHDL counter with skalp. Entity/architecture, `rising_edge`, ports, `skalp build`, basic simulation.

2. **[Combinational Logic](02-combinational-logic/)** — `process(all)`, `case/when`, concurrent assignments, `when...else`, `with...select`. Build a 4-to-1 multiplexer.

3. **[Clocked Processes and State Machines](03-processes-and-fsms/)** — Enumerated types, FSM patterns, prescalers, type casting. Build a timer and an I2C controller.

4. **[Generics, Records, and Arrays](04-generics-and-types/)** — Generic parameters, array types, register banks, edge detection. Build a GPIO controller.

5. **[Hierarchical Design](05-hierarchical-design/)** — Multi-entity designs, direct instantiation, port maps, internal signals. Connect a sender and receiver through a bus.

6. **[Testing VHDL with Rust](06-testing-with-rust/)** — The `Testbench` API: `set`, `clock`, `expect`. Waveform dumps, test organization, helper functions.

7. **[skalp Integration](07-skalp-integration/)** — `-- skalp:` pragmas for safety, CDC, and tracing. Formal verification. Mixed skalp+VHDL designs.

8. **[VHDL-2019 Features](08-vhdl-2019/)** — Interfaces, views, generic type parameters. skalp is one of the few free tools that supports these.

9. **[Real-World Project](09-real-world-project/)** — Capstone: a parameterized SPI master with generics, generate statements, a multi-state FSM, and a complete Rust test suite.

---

**Ready?** Start with [Chapter 1: Getting Started](01-getting-started/).
