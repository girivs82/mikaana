---
title: "skalp Tutorial"
summary: "Learn skalp by building a complete UART peripheral — from your first entity to clock domain crossings, safety annotations, and Rust-based testbenches."
---

## Learn skalp by Building Real Hardware

This tutorial takes you from zero skalp knowledge to a fully-featured UART peripheral with FIFOs, clock domain crossings, safety mechanisms, and a complete test suite. Each chapter introduces a language feature with a small standalone example, then applies it to the running UART project.

By the end, you'll have built:

- A **UART transmitter** with baud rate generation and FSM control
- A **UART receiver** with mid-bit sampling and edge detection
- **Parameterized FIFOs** with generic width and depth
- **Struct-based** configuration and status ports
- **Enum-driven** state machines with exhaustive pattern matching
- **Clock domain crossing** safety with dual-clock async FIFOs
- **Safety annotations** including TMR, trace, and breakpoint infrastructure
- A **Rust testbench** with full coverage of the UART peripheral

---

## Prerequisites

This tutorial assumes you already know how to design digital hardware. You should be comfortable with:

- RTL concepts: registers, combinational logic, clock edges, reset
- State machines, counters, shift registers
- Basic UART protocol (start bit, data bits, stop bit)
- Either SystemVerilog or VHDL (comparisons throughout use SystemVerilog)

You do **not** need to know Rust, though familiarity helps for Chapter 10 (testing). The tutorial explains Rust-specific concepts where they appear.

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

Create a new project for the tutorial:

```bash
skalp new uart-tutorial
cd uart-tutorial
```

This creates a project with `skalp.toml` and a `src/` directory. Each chapter adds files to this project.

---

## Chapters

1. **[Getting Started](01-getting-started/)** — Entities, signals, `on(clk.rise)`, and your first counter. The entity/impl split, port declarations, basic types.

2. **[State Machines — UART Transmitter](02-state-machines/)** — Build the UART TX with FSM states, baud rate timing, and shift register serialization.

3. **[UART Receiver](03-uart-receiver/)** — Mid-bit sampling, edge detection, bit reconstruction. The RX side of the UART.

4. **[Arrays and Generics — FIFO Buffering](04-arrays-and-generics/)** — Array types, generic parameters, `clog2()`. Build a parameterized FIFO and add buffering to the UART.

5. **[Const Generics and Parameterization](05-parameterization/)** — Generic defaults, compile-time computation, test vs. production parameters. Make the UART fully configurable.

6. **[Structs and Hierarchical Composition](06-structs-and-composition/)** — Struct definitions, struct ports, hierarchical instantiation. Clean up the UART with structured configuration.

7. **[Enums and Pattern Matching](07-enums-and-matching/)** — Enum types, `match` expressions, exhaustiveness checking. Refactor FSM states and add a command parser.

8. **[Clock Domain Crossing](08-clock-domain-crossing/)** — Clock lifetimes, CDC compile-time safety, dual-clock entities, async FIFOs. Make the UART dual-clock.

9. **[Safety and Annotations](09-safety-and-annotations/)** — `#[safety_mechanism]`, TMR voting, `#[trace]`, `#[breakpoint]`. Add safety infrastructure to the UART.

10. **[Testing and Verification](10-testing/)** — Rust testbench API, test organization, waveform generation. Build a complete test suite for the UART.

---

## What This Tutorial Doesn't Cover

This tutorial focuses on the skalp language and workflow. For deeper topics, see:

- **Compiler internals and architecture** — [skalp project page](/projects/skalp/)
- **Null Convention Logic (async circuits)** — [NCL blog post](/blog/null-convention-logic/)
- **Production design patterns** — [Design Patterns in Real skalp Code](/blog/skalp-design-patterns/)

---

**Ready?** Start with [Chapter 1: Getting Started](01-getting-started/).
