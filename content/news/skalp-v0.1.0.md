---
title: "skalp v0.1.0 Released"
date: 2026-02-22
summary: "First release of skalp — an intent-driven hardware description language with compile-time clock domain safety, built-in synthesis, and native FPGA place & route."
tags: ["skalp", "release", "hardware", "hdl"]
ShowToc: true
---

[skalp](/projects/skalp/) v0.1.0 is out — the first release of an intent-driven hardware description language that preserves design intent from algorithm to gates, with compile-time clock domain safety and progressive refinement.

Pre-built binaries are available for Linux, macOS, and Windows. [Get it on GitHub.](https://github.com/girivs82/skalp/releases/tag/v0.1.0)

---

## What's in v0.1.0

### Language

- Intent-first hardware design with progressive abstraction from dataflow to cycle-accurate RTL
- Clock domains as compile-time lifetimes — CDC bugs are caught by the compiler, not discovered at 3 AM
- Modern type system with traits, generics, const generics, and pattern matching
- Trait-based polymorphism for zero-duplication hardware component reuse
- Built-in verification: assertions, assumptions, and formal properties as first-class citizens
- Power intent attributes (`#[retention]`, `#[isolation]`, `#[level_shift]`, `#[pdc]`)
- Memory configuration attributes (block RAM, distributed, UltraRAM, register files)

### Compiler

- Multi-stage pipeline: Frontend → MIR → LIR → SIR → Backend
- Code generation targeting SystemVerilog, Verilog, and VHDL
- Hierarchical gate-level synthesis with per-instance optimization
- NCL asynchronous circuit support for clockless, delay-insensitive designs
- ML-guided logic synthesis with AIG-based optimization and learned pass ordering
- iCE40 FPGA backend with native place-and-route and programmer support
- Clock domain crossing analysis with automatic synchronizer generation

### Simulation

- Compiled CPU simulation via C++ code generation
- Gate-level CPU simulation
- GPU-accelerated simulation via Metal on macOS
- Debug breakpoints with conditions (`#[breakpoint]`)
- Signal tracing with automatic waveform export (`#[trace]`)

### Tooling

- `skalp build` — compile to HDL or gate-level netlists
- `skalp sim` — run simulations with CPU or GPU backends
- `skalp lint` — hardware-aware static analysis
- `skalp fmt` — code formatter
- `skalp new` — project scaffolding with starter templates
- `skalp verify` — formal verification via SVA generation
- Package manager with dependency management
- LSP server for VS Code, Vim, and Emacs

### Standard Library

- Bitwise operations (CLZ, CTZ, popcount, bit reversal)
- Math functions (trigonometric, exponential, logarithmic, roots)
- Vector operations (arithmetic, dot products, cross products, normalization)
- Fixed-point arithmetic with Q-format, saturation, and rounding
- Common hardware primitives (FIFO, counters, UART, I2C, SPI)

---

## Installation

### Pre-built binaries

| Platform | Binary |
|---|---|
| Linux x86_64 | [`skalp-linux-x86_64`](https://github.com/girivs82/skalp/releases/tag/v0.1.0) |
| macOS x86_64 | [`skalp-macos-x86_64`](https://github.com/girivs82/skalp/releases/tag/v0.1.0) |
| macOS ARM64 | [`skalp-macos-arm64`](https://github.com/girivs82/skalp/releases/tag/v0.1.0) |
| Windows x86_64 | [`skalp-windows-x86_64.exe`](https://github.com/girivs82/skalp/releases/tag/v0.1.0) |

Download the binary for your platform and add it to your `PATH`.

### From source

```bash
cargo install --git https://github.com/girivs82/skalp --tag v0.1.0
```

---

## Links

- [GitHub release](https://github.com/girivs82/skalp/releases/tag/v0.1.0)
- [Project page](/projects/skalp/)
- [Source code](https://github.com/girivs82/skalp)
