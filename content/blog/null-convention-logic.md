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

### Boundary-Only NCL: The Practical Middle Ground

The textbook version of NCL converts *every* signal to dual-rail and *every* gate to threshold logic. For an 8-bit adder, that means 16 input wires, 16 output wires, and every internal carry and sum wire doubled — plus threshold gates everywhere. The area overhead is real and it's the main reason NCL stays in academia.

skalp takes a different approach by default: **boundary-only NCL**. The idea is that you only need dual-rail encoding at the *interfaces* between pipeline stages — the points where the NCL handshaking protocol actually operates. Inside a pipeline stage, the logic is purely combinational: it doesn't matter whether it's built from threshold gates or standard gates, because within a single stage, all inputs arrive (via the dual-rail protocol) before any outputs are consumed (via completion detection).

The compilation works like this:

```
Dual-rail inputs → NclDecode → Standard combinational logic → NclEncode → Dual-rail outputs
                                                                              ↓
                                                                    Completion detection
```

1. **NclDecode** at the inputs: converts each dual-rail pair `(t, f)` back to a single-rail bit. The true rail *is* the data — when `t=1, f=0`, the bit is 1; when `t=0, f=1`, the bit is 0. During the NULL phase, both rails are 0, and the decoder holds its previous value (or outputs 0, depending on configuration).

2. **Standard logic** in the middle: the adder, multiplier, mux — whatever the entity computes — runs as ordinary combinational gates. AND gates, OR gates, XOR gates. No dual-rail, no threshold gates. This is the same logic you'd synthesize for a synchronous design.

3. **NclEncode** at the outputs: converts single-rail bits back to dual-rail pairs. A `1` becomes `(t=1, f=0)`, a `0` becomes `(t=0, f=1)`.

4. **Completion detection** on the outputs: monitors all output dual-rail pairs. When every pair has left the NULL state (at least one rail is high), the stage signals completion.

**Why does this still work as async?** The NCL protocol's correctness depends on two properties: (a) outputs don't transition until *all* inputs have arrived, and (b) outputs return to NULL before accepting new inputs. Boundary-only encoding preserves both:

- The upstream stage's completion detector ensures all inputs have transitioned to valid DATA before the downstream stage's decoder sees them. The combinational logic between decode and encode is just... logic. It settles in some bounded time, and the completion detector on the output side doesn't fire until all outputs are valid.
- During the NULL phase, the decoder sees NULL on all inputs, the combinational logic settles to whatever state those decoded values produce, and the encoder reflects that as NULL on the outputs.

The tradeoff: boundary-only NCL is **quasi-delay-insensitive** rather than fully delay-insensitive. It assumes that the combinational logic within a stage settles before the next DATA wavefront arrives. This is a weaker guarantee than full NCL, where every gate individually respects the protocol. But in practice, the handshaking between stages provides enough margin — the completion detector on the previous stage won't release the next wavefront until all outputs are valid, and the combinational settling time is bounded.

The gate count reduction is dramatic. An 8-bit adder in full NCL needs ~200 threshold gates. In boundary-only mode, it needs the same ~20 standard gates as a synchronous adder, plus ~20 gates for encode/decode/completion at the boundaries. That's roughly 2x instead of 10x.

For designs that *need* full delay-insensitivity — radiation-hardened, ultra-high-reliability, or circuits where you genuinely can't bound combinational delay — skalp supports full dual-rail expansion as a compile-time option. Every internal signal becomes a dual-rail pair, every gate becomes threshold logic. You pay the area cost, but you get the strongest timing guarantee.

### The NCL Optimization Pipeline

After expansion (boundary or full), the compiler runs six optimization passes:

1. **Constant propagation** — fold known DATA values through gates
2. **Idempotent collapse** — `TH12(a, a) → a` and `TH22(a, a) → a` (5-15% reduction)
3. **NOT propagation** — push rail swaps through the circuit (free in NCL — just wire reassignment)
4. **Threshold gate merging** — `TH22(TH22(...), TH22(...)) → TH44(...)` (20-50% reduction)
5. **Completion sharing** — identical completion trees across pipeline stages are shared, not duplicated (30-70% reduction)
6. **Dead rail elimination** — remove unused dual-rail signals

### Synthesis: Threshold Gates from Standard Cells

Here's a practical problem: if you're targeting a standard FPGA or ASIC library, there are no TH22 or TH12 cells in the library. Foundries don't ship threshold gates. So how do you actually build NCL circuits on real hardware?

The answer is that every threshold gate can be decomposed into standard logic gates plus a feedback path for hysteresis.

The C-element (TH22) — the most important threshold gate — is implemented as:

```
Q = (A & B) | (Q & (A | B))
```

Read it in two parts:
- `(A & B)`: when both inputs are high, output goes high (threshold met)
- `(Q & (A | B))`: when the output is already high and at least one input is still high, output *stays* high (hysteresis/hold)
- When both A and B are low, both terms are 0, so Q goes low (reset)

This is just an AND gate, an OR gate, another AND gate, a final OR gate, and a feedback wire from the output. Standard cells. No special library needed.

Similarly, TH12 decomposes to:

```
Q = A | B | (Q & (A | B))
```

Which simplifies to just `A | B` with a feedback latch — though in practice, a plain OR gate works for the true-rail side because the NCL protocol guarantees that once a rail goes high, it stays high until the NULL wavefront clears everything.

The technology mapper in skalp handles this automatically. When the target library has dedicated C-element cells (some ASIC libraries do), it uses them directly — they're smaller and faster than the decomposed version. When it doesn't (most FPGAs, many ASIC libraries), it synthesizes the feedback circuit from standard gates. The NCL design works either way; you just get better area and timing with dedicated cells.

For FPGAs specifically, a TH22 maps to a single LUT4 plus a feedback path through the flip-flop in the same logic cell — fitting neatly into one CLB. The LUT implements the combinational function, and the flip-flop provides the state for hysteresis. This is actually quite efficient on iCE40 and similar architectures.

The full flow — AIG optimization, placement, PathFinder routing, and bitstream generation — works with NCL designs. The synthesis engine treats threshold gates as standard cells with feedback; the placer and router don't need to know they're NCL.

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
