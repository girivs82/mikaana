# Aggregator Submission Drafts for SKALP v0.1.1

---

## 1. Hacker News — Show HN

### Title

Show HN: SKALP – An intent-driven hardware language with clock domains as types

### Author Comment

I've been working on SKALP, a hardware description language written in Rust, and just shipped v0.1.1 with pre-built binaries for Linux, macOS, and Windows.

The core idea: hardware design tools throw away too much intent too early. You write a state machine, but by the time it hits synthesis, the tool only sees a bag of gates and has to reverse-engineer what you meant. SKALP keeps that intent through four intermediate representations — from a high-level algorithmic IR down to gate-level netlist — so each compilation stage has the context it needs to make good decisions.

The thing I'm most interested in technically is clock domain crossings as compile-time types. If you've worked on multi-clock designs, you know CDC bugs are some of the hardest to find. In SKALP, clock domains are parameterized types (think Rust lifetimes but for clocks), so connecting signals across domains without a proper synchronizer is a compiler error, not a simulation surprise at 3am.

Other pieces that might be interesting: GPU-accelerated fault simulation via Metal (~11M fault-cycle sims/sec on M4 Max), integrated P&R targeting iCE40 FPGAs, built-in formal verification, and ISO 26262 FMEDA generation. The syntax is expression-based and Rust-influenced — traits, generics, pattern matching.

It's still early. The iCE40 backend works but coverage is limited, and the GPU path is macOS-only for now. The codebase is ~290K lines across 23 crates.

Website: https://mikaana.com/projects/skalp/
GitHub: https://github.com/girivs82/skalp
Tutorial (10 chapters): https://mikaana.com/tutorial/
Blog post on the IR pipeline: https://mikaana.com/blog/skalp-ir-pipeline/

---

## 2. Reddit r/FPGA

### Title

SKALP v0.1.1: A new HDL with compile-time clock domain checking, integrated synthesis, and iCE40 P&R — looking for feedback from FPGA engineers

### Body

I've been building an HDL called SKALP and just released v0.1.1. I wanted to share it here because the FPGA community is who I built this for, and I'd genuinely appreciate feedback on whether the problems I'm solving are the ones that actually hurt.

**What is it?** SKALP is an intent-driven hardware description language with its own compiler, simulator, synthesis engine, and place-and-route backend (currently targeting iCE40). It's written in Rust, and the syntax borrows from Rust too — expression-based, with traits, generics, and pattern matching.

**What problems does it attack?**

- **CDC bugs at compile time.** Clock domains are types in SKALP, parameterized like Rust lifetimes. If you try to read a signal from `clk_a` domain in a `clk_b` process without going through a synchronizer, the compiler rejects it. No lint tool, no separate CDC checker — it's structural.

- **Tool fragmentation.** A typical FPGA flow involves an HDL, a simulator (maybe two), a synthesis tool, a P&R tool, a formal tool, a lint tool, and a bunch of TCL glue. SKALP integrates all of these. `skalp build` goes from source to bitstream for iCE40.

- **Lost intent.** SystemVerilog and VHDL flatten everything to RTL very early. SKALP uses four IRs internally — high-level algorithmic, structured, RTL, and gate-level — so synthesis can exploit knowledge about FSMs, pipelines, and dataflow that would otherwise be lost.

- **Fault analysis.** If you work in automotive or safety-critical, you know FMEDA generation is painful. SKALP has built-in ISO 26262 fault injection with GPU-accelerated simulation (~11M fault-cycle sims/sec on Apple Silicon via Metal).

**What actually works today?** The compiler, behavioral simulation, gate-level simulation, iCE40 synthesis and P&R, and formal verification all work. The iCE40 backend is real but coverage of primitives is still limited. GPU simulation is macOS-only (Metal). There's no Xilinx or Intel target yet.

**What it's not.** It's not a drop-in replacement for SystemVerilog. It's a new language with a new toolchain. If you need to interface with existing IP cores in Verilog, there's no interop story yet.

I wrote a detailed blog post on how the four-IR pipeline works: https://mikaana.com/blog/skalp-ir-pipeline/

Pre-built binaries, a 10-chapter tutorial, and the source are all available:
- GitHub: https://github.com/girivs82/skalp
- Tutorial: https://mikaana.com/tutorial/
- Website: https://mikaana.com/projects/skalp/

Would love to hear what you think — especially what's missing that would make you actually try it on a real project.

---

## 3. Reddit r/rust

### Title

SKALP: A hardware description language in Rust (~221K lines, 24 crates) — clock domains as compile-time types, four-IR compiler pipeline, GPU-accelerated simulation

### Body

I just released v0.1.1 of SKALP, a hardware description language and integrated toolchain written entirely in Rust. Thought this community might find the compiler architecture interesting.

**The type system idea.** In hardware, clock domain crossings (passing data between different clock frequencies) are a notorious source of bugs. SKALP models clock domains as compile-time type parameters — conceptually similar to how Rust uses lifetimes to prevent use-after-free. A signal typed as `Signal<ClkA, u8>` can't be used in a `ClkB` process without going through a synchronizer primitive. The compiler enforces this statically.

**Compiler pipeline.** SKALP compiles through four IRs: a high-level algorithmic representation, a structured IR (loops, FSMs preserved), an RTL IR, and a gate-level netlist. Each lowering pass has access to intent that's typically discarded in traditional hardware compilers. I wrote about this in detail: https://mikaana.com/blog/skalp-ir-pipeline/

**Rust-specific things that worked well:**
- The `enum`-heavy IR representations map naturally to Rust's algebraic types
- `rayon` for parallel synthesis and simulation scheduling
- The trait system for abstracting over simulation backends (CPU compiled, CPU gate-level, GPU via Metal)
- `ouroboros` for self-referential compiled library caching (though it fights with tarpaulin)

**What the codebase looks like:** ~290K lines across 23 crates. Compiler frontend, four IR stages, behavioral simulator, gate-level simulator, C++ codegen backend, Metal GPU backend, iCE40 FPGA synthesis and P&R, formal verification via Z3, and ISO 26262 fault analysis.

Pre-built binaries for Linux, macOS (x86_64 + ARM64), and Windows are up.

- GitHub: https://github.com/girivs82/skalp
- Tutorial: https://mikaana.com/tutorial/
- Design patterns blog post: https://mikaana.com/blog/skalp-design-patterns/

---

## 4. Reddit r/programming

### Title

SKALP: A hardware compiler that treats clock domains like Rust treats lifetimes — CDC bugs become compiler errors

### Body

I've been building a hardware description language called SKALP and just shipped v0.1.1. The short version: it's an integrated toolchain — compiler, simulator, synthesis, place-and-route, formal verification — for designing digital circuits, written in Rust.

**Why build a new HDL?** The dominant hardware languages (Verilog, VHDL) are from the 1980s. They're essentially simulation languages that we've bolted synthesis onto. You describe behavior, the tools infer structure, and a whole class of bugs lives in the gap between what you meant and what the tools understood.

SKALP tries to close that gap. The compiler preserves design intent through four intermediate representations, from algorithm-level down to individual gates. Each compilation stage can see what you actually meant — "this is a pipeline," "this is an FSM" — instead of just a flat netlist.

**The type system trick.** Clock domain crossing bugs (signals moving between different clock frequencies without proper synchronization) are one of the hardest problems in hardware. SKALP encodes clock domains as compile-time types, so the compiler catches these structurally — similar to how Rust's borrow checker catches memory bugs.

The toolchain targets iCE40 FPGAs end-to-end and includes GPU-accelerated fault simulation for safety-critical applications.

- GitHub: https://github.com/girivs82/skalp
- How the IR pipeline works: https://mikaana.com/blog/skalp-ir-pipeline/
- Tutorial: https://mikaana.com/tutorial/

---

## 5. lobste.rs

### Title

SKALP: Intent-driven hardware description language with compile-time clock domain checking

### URL

https://mikaana.com/projects/skalp/

### Suggested Tags

`compilers`, `rust`, `hardware`

### Alternative Title (if linking to blog post instead)

Four IRs Deep: How SKALP Compiles Hardware

### Alternative URL

https://mikaana.com/blog/skalp-ir-pipeline/

### Alternative Tags

`compilers`, `rust`, `hardware`, `plt`
