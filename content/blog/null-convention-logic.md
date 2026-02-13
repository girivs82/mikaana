---
title: "Null Convention Logic: Hardware That Doesn't Need a Clock"
date: 2025-06-15
summary: "An introduction to Null Convention Logic — asynchronous, self-timed digital circuits that use dual-rail encoding and threshold gates instead of a global clock. Why it matters, how it works, and why skalp is the first toolchain to support it end-to-end."
tags: ["ncl", "async", "hardware", "fpga"]
ShowToc: true
---

Every digital circuit you've used today is synchronous — driven by a global clock that tells every flip-flop when to sample its input. This works, but it comes with a set of problems that get worse as designs get larger: clock distribution, timing closure, power consumption from clock trees, and an entire class of bugs (setup/hold violations, metastability) that exist only because we chose to organize computation around a periodic edge.

What if the circuit could just... compute when the data is ready?

That's the idea behind **Null Convention Logic (NCL)** — a design methodology for asynchronous, self-timed digital circuits where data flow drives computation instead of a clock. No clock means no clock distribution problems, no timing closure, no setup/hold violations, and inherent properties that make NCL attractive for safety-critical and radiation-hardened applications.

This post explains how NCL works from first principles, why it's been stuck in academia for decades, and what it would take to make it practical.

---

## The Problem with Clocks

In synchronous design, the clock is a global coordinator. Every register samples simultaneously, and the designer's job is to ensure that combinational logic between registers settles before the next clock edge. This constraint — **timing closure** — dominates the later stages of any serious hardware project.

The problems compound:

**Clock distribution.** Getting a clean clock edge to every flip-flop in a large chip requires a carefully balanced tree of buffers. Clock tree synthesis is its own sub-discipline. Skew between branches creates timing margin that eats into your frequency budget.

**Power.** The clock tree is the single largest source of dynamic power in most digital designs — it toggles every cycle whether there's useful work or not. Clock gating helps, but adds complexity.

**Timing violations.** If a combinational path is too slow, the downstream register samples garbage. These bugs are intermittent, depend on temperature and voltage, and are notoriously difficult to reproduce in simulation.

**Modularity.** Connecting two synchronous modules requires either the same clock (limiting reuse) or explicit clock domain crossing logic (error-prone and expensive to verify).

NCL sidesteps all of these by eliminating the clock entirely.

---

## Dual-Rail Encoding: Data That Knows When It's Ready

The fundamental insight of NCL is encoding validity into the data itself. Instead of a single wire per bit, NCL uses **two wires** — a dual-rail pair:

| True Rail (t) | False Rail (f) | Meaning |
|:---:|:---:|:---|
| 0 | 0 | **NULL** — no data present (spacer) |
| 1 | 0 | **DATA 1** — logical true |
| 0 | 1 | **DATA 0** — logical false |
| 1 | 1 | **INVALID** — should never occur (indicates a fault) |

A single logical bit becomes two physical wires. An 8-bit bus becomes 16 wires. The cost is real — roughly 2x the wiring — but the encoding carries its own validity: you can look at any dual-rail signal and know whether it contains data or is empty.

Computation alternates between two phases:

1. **DATA wavefront**: Input signals transition from NULL to valid DATA states. Logic gates evaluate and produce valid outputs. When all outputs are valid, the computation is complete.
2. **NULL wavefront**: All signals return to NULL (0,0), resetting the pipeline for the next computation.

There's no clock edge deciding when to sample. The circuit is done when the data says it's done.

---

## Threshold Gates: Logic with Memory

Standard logic gates (AND, OR, NOT) don't work correctly in NCL because they can't distinguish between "input is logic 0" and "input hasn't arrived yet." An AND gate outputs 0 when either input is 0 — but in NCL, we need the gate to *wait* until both inputs have arrived before producing output.

NCL uses **threshold gates** — gates with hysteresis that hold their previous output until a threshold condition is met:

**TH*mn*** — an *m*-of-*n* threshold gate:
- Output goes **HIGH** when at least *m* of its *n* inputs are HIGH
- Output goes **LOW** when **all** inputs are LOW
- Otherwise, the output **holds its previous value**

The "hold" behavior is the key. It means the gate waits for enough inputs to arrive before committing to an output, and it doesn't reset until the NULL wavefront clears everything.

The two most important threshold gates:

**TH12** (1-of-2): Goes high when *either* input is high. Resets when both are low. This is an OR gate with hysteresis.

**TH22** (2-of-2): Goes high when *both* inputs are high. Resets when both are low. This is the **C-element** (Muller gate) — the fundamental building block of NCL. It's like an AND gate that remembers its output during transitions.

```
TH22 (C-element) truth table with state:
  A  B  | prev=0 | prev=1
  0  0  |   0    |   0     (reset: all inputs low → output low)
  0  1  |   0    |   1     (hold previous)
  1  0  |   0    |   1     (hold previous)
  1  1  |   1    |   1     (threshold met → output high)
```

The hysteresis prevents glitches: a TH22 won't pulse high if input A arrives before input B. It waits.

---

## Building Logic from Threshold Gates

NCL logic operates on dual-rail pairs. Each operation takes dual-rail inputs and produces dual-rail outputs, using threshold gates to implement the logic while respecting the NULL/DATA protocol.

### NCL AND

To compute `AND(a, b)` where `a = (a_t, a_f)` and `b = (b_t, b_f)`:

```
true_rail:  result_t = TH22(a_t, b_t)   — both true → result true
false_rail: result_f = TH12(a_f, b_f)   — either false → result false
```

This maps directly to the Boolean definition: AND is true only when both inputs are true, and false when either input is false. The threshold gates ensure the output waits for valid inputs.

### NCL OR

```
true_rail:  result_t = TH12(a_t, b_t)   — either true → result true
false_rail: result_f = TH22(a_f, b_f)   — both false → result false
```

The dual of AND — swap which rail gets the TH22 and which gets the TH12.

### NCL NOT

```
result_t = a_f
result_f = a_t
```

Inversion in NCL is free — just swap the rails. No gates, no delay.

### NCL XOR

```
result_t = TH22(TH12(a_t, b_f), TH12(a_f, b_t))
result_f = TH22(TH12(a_t, b_t), TH12(a_f, b_f))
```

XOR requires four TH12 gates and two TH22 gates. More expensive than in synchronous logic, but still composable from the same primitives.

---

## Completion Detection: Knowing When You're Done

In synchronous circuits, the clock tells you when computation is complete. In NCL, you need a different mechanism: **completion detection**.

A completion detector monitors all output signals of a pipeline stage. When every dual-rail output has transitioned from NULL to a valid DATA state (either `(1,0)` or `(0,1)`), the stage is complete.

The simplest implementation: for each dual-rail pair, OR the true and false rails (if either rail is high, that bit has arrived). Then AND all the per-bit results together. When the AND output goes high, all bits are valid.

```
For outputs (o0_t, o0_f), (o1_t, o1_f), ..., (oN_t, oN_f):

  bit_valid[i] = o_t[i] OR o_f[i]
  all_complete = bit_valid[0] AND bit_valid[1] AND ... AND bit_valid[N]
```

In practice, this is built as a tree of threshold gates for balanced delay. The completion signal feeds back to the previous stage as an acknowledgment: "I've received your data, you can send NULL now."

---

## The NCL Pipeline

An NCL pipeline stages computation using **registration elements** — dual-rail latches controlled by completion signals rather than a clock:

```
Stage 1 Logic → Completion Detect → Latch → Stage 2 Logic → Completion Detect → Latch → ...
                     ↑                                            ↑
                     └── Acknowledgment from Stage 2              └── Acknowledgment from Stage 3
```

The handshaking protocol:

1. Stage 1 outputs transition to DATA. Stage 2's completion detector sees all outputs valid.
2. Stage 2 latches the data and signals acknowledgment to Stage 1.
3. Stage 1 transitions to NULL. The NULL wavefront propagates through Stage 1 logic.
4. Stage 2's completion detector sees NULL (all outputs return to `(0,0)`).
5. Stage 2 sends acknowledgment that it has received NULL. Ready for next DATA wavefront.

Each stage operates at its own speed. A fast stage doesn't wait for a slow one — it simply produces its output and waits for acknowledgment. A slow stage takes longer but doesn't cause a timing violation. The pipeline is **elastic**: throughput is limited by the slowest stage, but correctness is guaranteed regardless of individual stage delays.

---

## Why NCL Stayed in Academia

If NCL solves so many problems, why isn't everyone using it?

**Area overhead.** Dual-rail encoding roughly doubles the wiring. Threshold gates require more transistors than simple CMOS gates. Completion detection adds overhead at every pipeline stage. A typical NCL design uses 2-3x the area of an equivalent synchronous design.

**No tooling.** This is the real barrier. The entire EDA ecosystem — synthesis tools, place-and-route, timing analysis, simulation, verification — is built for synchronous design. There are no commercial NCL synthesis tools. If you want to build an NCL circuit, you're hand-instantiating threshold gates in structural Verilog or writing your own CAD tools.

**No standard cell libraries.** Foundries provide standard cell libraries optimized for synchronous design. C-elements and threshold gates aren't in the standard offerings. You need custom cells or have to build them from standard gates (losing the area advantage of dedicated implementations).

**Verification gap.** Formal verification, equivalence checking, and static timing analysis all assume synchronous semantics. Verifying an NCL design requires different methodologies that don't exist in commercial tools.

**Inertia.** Engineers know synchronous design. Universities teach synchronous design. IP cores are synchronous. The ecosystem momentum is enormous.

The result is a catch-22: NCL stays niche because there's no tooling, and no one builds tooling because NCL is niche.

---

## Where NCL Makes Sense Despite the Cost

The area overhead is real, but there are domains where NCL's properties justify it:

**Safety-critical systems (ISO 26262, DO-254).** Dual-rail encoding is inherently self-checking — the invalid state `(1,1)` can only occur if something has gone wrong. A transient fault (radiation, voltage glitch) that flips a single rail creates a detectable invalid state rather than a silent data corruption. This gives NCL circuits natural diagnostic coverage that synchronous circuits need explicit safety mechanisms to achieve.

**Radiation-hardened designs.** Space and aviation electronics face single-event upsets from cosmic rays. Synchronous designs mitigate these with triple modular redundancy (3x area). NCL's dual-rail encoding detects many SEUs at 2x area, with the detection built into the data representation rather than bolted on.

**Low-EMI applications.** Synchronous circuits concentrate their switching energy at the clock frequency and its harmonics, creating predictable electromagnetic emissions. NCL circuits spread their switching across time, producing a flatter emission spectrum. This matters for mixed-signal designs where digital noise corrupts analog circuits, and for regulatory compliance.

**Variable-latency computation.** When different inputs take different amounts of time to process (like a cache hit vs. miss, or an early-terminating multiplier), synchronous designs must wait for the worst case every cycle. NCL naturally produces output as soon as the computation finishes — average-case performance instead of worst-case.

**Security.** Side-channel attacks on synchronous circuits exploit the timing correlation between data and power consumption. NCL's data-driven timing provides natural resistance to timing-based side channels.

---

## NCL Support in skalp

This is where I should mention that skalp — the HDL I'm building — has first-class NCL support. To my knowledge, it's the only tooling ecosystem that takes NCL from language through synthesis to bitstream.

### The Language

NCL entities are declared with the `async` keyword:

```
async entity NclAdder {
    in a: bit[8]
    in b: bit[8]
    out sum: bit[8]
    out carry: bit[1]
}

impl NclAdder {
    let result: bit[9] = a +: b
    sum = result[7:0]
    carry = result[8]
}
```

You write the logic in terms of single-rail operations — the same `+`, `&`, `|`, `^` as synchronous code. The compiler handles dual-rail expansion, threshold gate mapping, and completion detection generation. Pipeline stages are marked with `barrier` statements:

```
async entity NclPipeline {
    in data: bit[32]
    out result: bit[32]
}

impl NclPipeline {
    let stage1 = transform_a(data)
    barrier  // completion detection inserted here
    let stage2 = transform_b(stage1)
    barrier
    result = transform_c(stage2)
}
```

### The Compilation

The compiler uses a **boundary-only** expansion by default: inputs are decoded from dual-rail, internal logic runs as standard combinational gates, and outputs are re-encoded to dual-rail with completion detection on the primary outputs. This dramatically reduces gate count compared to full dual-rail expansion of every internal signal.

For designs that need complete delay-insensitivity internally (safety-critical, radiation-hardened), full dual-rail expansion is available as an option — every internal signal becomes a dual-rail pair, every operation expands to threshold gate combinations.

The NCL expansion pass then runs six optimization passes:

1. **Constant propagation** — fold known DATA values through gates
2. **Idempotent collapse** — `TH12(a, a) → a` and `TH22(a, a) → a`
3. **NOT propagation** — push rail swaps through the circuit (free in NCL)
4. **Threshold gate merging** — `TH22(TH22(...), TH22(...)) → TH44(...)`, reducing gate count by 20-50%
5. **Completion sharing** — identical completion trees are shared, not duplicated
6. **Dead rail elimination** — remove unused dual-rail signals

### Synthesis and Place & Route

The technology mapper recognizes NCL primitives and maps them to library cells — TH22 to C-elements when available, or synthesizes C-elements from standard gates (`Q = (a & b) | (Q & (a | b))`) when they aren't. The full flow through AIG optimization, placement, PathFinder routing, and iCE40 bitstream generation works with NCL designs.

### Safety Integration

NCL elements carry FIT (Failure In Time) rates in the gate netlist, flowing through to FMEDA analysis. The fault injection system can inject faults into threshold gates and measure whether the dual-rail encoding detects them — quantifying the inherent diagnostic coverage of NCL designs rather than assuming it.

---

## Further Reading

If you want to go deeper into NCL theory and practice:

- **Fant, K.M. & Brandt, S.A.** — *NULL Convention Logic: A Complete and Consistent Logic for Asynchronous Digital Circuit Synthesis* (the original 1996 paper)
- **Smith, S.C.** — *NCL Design and Optimization* (comprehensive textbook)
- **Theseus Logic** — Karl Fant's company, now defunct, built commercial NCL processors
- **Martin, A.J.** — Work on delay-insensitive circuits at Caltech, including the async MIPS processor

NCL is one of several asynchronous methodologies (others include quasi-delay-insensitive design, micropipelines, and bundled-data). Each makes different tradeoffs between area, performance, and timing assumptions. NCL's specific contribution is the threshold gate + dual-rail combination that achieves delay-insensitivity without explicit timing constraints.
