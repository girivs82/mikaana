---
title: "skalp v0.2.0 Released"
date: 2026-03-04
summary: "skalp v0.2.0 adds VHDL as a first-class input language — write VHDL, get the same synthesis pipeline, simulation, and verification that skalp programs get."
tags: ["skalp", "release", "hardware", "hdl", "vhdl"]
ShowToc: true
---

[skalp](/projects/skalp/) v0.2.0 is out. The headline feature is a **VHDL frontend** — write standard VHDL-2008/2019 and feed it through the same synthesis, simulation, and verification pipeline as native skalp code.

Pre-built binaries are available for Linux, macOS, and Windows. [Get it on GitHub.](https://github.com/girivs82/skalp/releases/tag/v0.2.0)

---

## VHDL Frontend

skalp now accepts VHDL as a first-class input language. The new `skalp-vhdl` crate provides a complete lexer, parser, and HIR lowering pass for a synthesizable subset of VHDL-2008/2019:

- **Full synthesizable subset** — entities, architectures, processes, signals, variables, generate statements, and component instantiation
- **VHDL-2019 features** — interfaces, mode views, generic type parameters, and generic package instantiation
- **Hierarchical designs** — multi-entity elaboration with end-to-end behavioral simulation
- **VHDL-to-SystemVerilog transpilation** — direct conversion through the HIR layer
- **Testbench support** — Rust async testbenches work with VHDL designs, same as skalp sources

Once parsed, VHDL designs enter the same MIR → LIR → SIR → backend pipeline as native skalp, so every optimization, analysis, and code generation pass applies equally.

## Diagnostics

Error messages now use `codespan-reporting` for rustc-style diagnostics — source spans, colored labels, and fix suggestions rendered inline. This applies to both skalp and VHDL sources.

## Tooling Improvements

- **VHDL LSP support** — the language server handles VHDL files with semantic token highlighting and schematic support
- **VHDL formatter** — Wadler-Lindig pretty-printing for VHDL source
- **skalp source formatter** — `skalp fmt` rewritten with Wadler-Lindig pretty-printing
- **HIR codegen** — new code generation path from HIR to skalp, VHDL, and SystemVerilog with comment preservation, entity deduplication, and per-entity file output
- **InputTiming** — testbench control for waveform-aligned input drives

## Reliability

- Stack overflow protection via `stacker` in the SIR and MIR passes
- Cross-process compilation serialization (`flock`) replacing in-process `Mutex`
- Compiler fingerprint in SIR cache key to prevent stale cache hits
- C++ and Metal shader compilation serialization to prevent OOM/SIGKILL

## Bug Fixes

- Fixed async reset pattern causing double-increment in simulator
- Fixed dynamic array read/write in hierarchical elaboration
- Fixed multi-entity elaboration bugs causing simulation failures
- Fixed signal initial value propagation in MIR→SIR
- Fixed struct field access crash, CDC lifetime parameters, widening add operator
- Fixed parser infinite loop on real-world VHDL
- Fixed all VHDL parser gaps found across 6 stress-test projects
- Numerous codegen and `always_ff` generation fixes

See the [full changelog](https://github.com/girivs82/skalp/blob/main/CHANGELOG.md) for details.

---

## Installation

### Pre-built binaries

| Platform | Binary |
|---|---|
| Linux x86_64 | [`skalp-linux-x86_64`](https://github.com/girivs82/skalp/releases/tag/v0.2.0) |
| macOS x86_64 | [`skalp-macos-x86_64`](https://github.com/girivs82/skalp/releases/tag/v0.2.0) |
| macOS ARM64 | [`skalp-macos-arm64`](https://github.com/girivs82/skalp/releases/tag/v0.2.0) |
| Windows x86_64 | [`skalp-windows-x86_64.exe`](https://github.com/girivs82/skalp/releases/tag/v0.2.0) |

Download the binary for your platform and add it to your `PATH`.

### From source

```bash
cargo install --git https://github.com/girivs82/skalp --tag v0.2.0
```

---

## Links

- [GitHub release](https://github.com/girivs82/skalp/releases/tag/v0.2.0)
- [Project page](/projects/skalp/)
- [Source code](https://github.com/girivs82/skalp)
