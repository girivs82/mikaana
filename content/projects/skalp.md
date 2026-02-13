---
title: "skalp — Intent-Driven Hardware Description Language"
date: 2025-01-01
summary: "A modern HDL that bridges high-level algorithm design and low-level RTL, with compile-time clock domain safety and intent preservation. Written in Rust."
tags: ["hardware", "hdl", "rust", "fpga", "synthesis"]
ShowToc: true
---

## What is skalp?

**skalp** (from Sanskrit *संकल्पना — Sankalpana*, meaning "conception with purpose") is a modern hardware description language that bridges the gap between high-level algorithm design and low-level RTL implementation. It preserves design intent throughout the entire compilation pipeline — from conception to synthesis.

[GitHub](https://github.com/girivs82/skalp)

## The Problem

Traditional HDLs force a painful choice: write tedious low-level RTL (Verilog/VHDL) with full control, or use HLS tools that give you abstraction but unpredictable results. Both lose your design intent along the way.

skalp addresses:

- **Intent loss** — design goals disappear during implementation in traditional flows
- **Clock domain chaos** — CDC bugs discovered late, never at compile time
- **The abstraction cliff** — no middle ground between RTL tedium and HLS unpredictability
- **Weak type safety** — silent truncations, implicit conversions, confusing `reg`/`wire`/`logic`
- **Verification as afterthought** — correctness bolted on, not built in

## Architecture

skalp uses a multi-layer IR approach (similar to LLVM) with progressive lowering:

```
SKALP Source (.sk)
    ↓
Frontend (Lexer → Parser → Type Checking)
    ↓
HIR — High-level IR, intent preserved
    ↓
MIR — Cycle-accurate, architecture-independent
    ↓
LIR — Netlist with target primitives
    ↓
SIR — Simulation IR, GPU-optimized
    ↓
Backends: SystemVerilog · VHDL · Verilog · FPGA Bitstream
```

Intent is preserved at every stage. Optimization passes are guided by your declared goals, not heuristics.

## Language Features

**Clock domains as types** — CDC violations caught at compile time, not post-hoc:
```rust
signal<'fast_clk>[8]   // clock domain is part of the type
synchronize(data)       // explicit CDC crossing
```

**Intent declarations** — tell the compiler what you want, not just what to build:
```rust
entity Accelerator {
    in data: stream<'clk>[32]
    out result: stream<'clk>[32]
} with intent {
    throughput: 100M_samples_per_sec,
    architecture: systolic_array,
    optimization: balanced(speed: 0.7, area: 0.3)
}
```

**Modern abstractions** — traits, generics, const generics, pattern matching:
```rust
entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in clk: clock
    in rst: reset(active_high)
    in wr_en: bit
    in wr_data: bit[WIDTH]
    out full: bit
    in rd_en: bit
    out rd_data: bit[WIDTH]
    out empty: bit
}
```

**Strong type system** — FP16/32/64 (IEEE 754), fixed-point, bit vectors, generic vector types, stream types. No silent truncations.

## Tool Ecosystem

| Tool | Purpose |
|------|---------|
| `skalp build` | Compiler & build tool |
| `skalp sim` | GPU-accelerated simulation (Metal on macOS) |
| `skalp fmt` | Code formatter |
| `skalp lint` | Hardware-aware static analyzer (10 lint categories) |
| `skalp-lsp` | Language server for VS Code / Neovim |
| Package manager | Dependency resolution, add/remove/search |

## Standard Library

48+ built-in operations across:

- **Bitops** — clz, ctz, popcount, bitreverse, parity
- **Floating-point** — min, max, abs, clamp, lerp, FMA, sqrt, rsqrt
- **Vector** — dot, cross, normalize, reflect, project
- **DSP/Graphics** — Phong/Blinn-Phong shading primitives

## Synthesis Targets

- **Code generation** — SystemVerilog, VHDL, Verilog
- **FPGA** — native place & route for iCE40-HX8K, iCE40-UP5K
- **ASIC** — dedicated synthesis flow
- **Optimization presets** — quick, balanced, full, timing, area
- **ML-guided** pass ordering (experimental, ONNX)

## Safety & Verification

- **ISO 26262** functional safety analysis (FMEDA, ASIL levels A–D)
- **Formal verification** and model checking
- **Property-based testing** for hardware designs
- **Assertions** as first-class language constructs

## How It Compares

| | skalp | SystemVerilog | VHDL | Chisel |
|---|---|---|---|---|
| Type safety | Strong | Weak | Strong | Strong |
| Clock domain safety | Compile-time | None | None | None |
| Intent preservation | First-class | None | None | None |
| Modern abstractions | Traits, generics | Limited | None | Scala-based |
| GPU simulation | Metal (macOS) | No | No | No |

## By the Numbers

- **~221K lines** of Rust across 24 workspace crates
- **1,090+ commits** of active development
- **78 documentation files** including full language spec and compiler architecture docs
- **38+ example designs** from counters to AXI4-Lite controllers

## Examples

The repo includes real-world examples:

- FIFO buffers
- UART transmitter
- SPI master
- I2C master
- Memory arbiter
- AXI4-Lite interface
- Register file
- ALU
- Null Convention Logic (async circuits)
- Graphics pipeline components
