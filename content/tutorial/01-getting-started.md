---
title: "Chapter 1: Getting Started"
date: 2025-07-15
summary: "Your first skalp entity — an 8-bit counter that introduces the entity/impl split, port declarations, signal types, sequential logic with on(clk.rise), and combinational assignment."
tags: ["skalp", "tutorial", "hdl", "hardware"]
weight: 1
ShowToc: true
---

## What This Chapter Teaches

Every skalp design starts with two constructs: an **entity** that declares the hardware interface, and an **impl** that defines the behavior. If you have written hardware in SystemVerilog or VHDL, you already know the concept — skalp just separates it more explicitly and removes the boilerplate.

By the end of this chapter you will understand:

- How `entity` declares ports (inputs and outputs) without any logic
- How `impl` contains the actual behavior for that entity
- The built-in types: `clock`, `reset`, `nat[N]` (unsigned N-bit), `bit[N]` (N-bit vector)
- How `signal` declares internal state inside an impl
- How `on(clk.rise)` defines sequential logic (registered, edge-triggered)
- How bare assignments like `name = expr` define combinational logic
- That skalp has no wire/reg distinction — the compiler determines what is registered and what is combinational from how you use it
- That forward references work for combinational signals — you can use a signal before you define it

These are the building blocks of every skalp design. The UART project you build across this tutorial starts here with a simple counter.

---

## Standalone Example: 8-Bit Counter

Let us build a counter with an enable input and an overflow output. This is small enough to see every piece of the entity/impl pattern at once.

Create a file called `src/counter.sk`:

```
// An 8-bit counter with enable and overflow detection.
//
// When enable is high, the counter increments on every rising
// clock edge. When it wraps from 255 back to 0, the overflow
// output pulses high for one cycle.

entity Counter {
    in clk: clock,
    in rst: reset,
    in enable: bit[1],
    out count: nat[8],
    out overflow: bit[1]
}

impl Counter {
    // Internal state: the register that holds the count value.
    // "signal" declares a value that persists across clock cycles
    // when assigned inside an on() block.
    signal count_reg: nat[8]

    // Sequential logic — this block runs on every rising edge of clk.
    // Everything assigned inside on(clk.rise) becomes a register.
    on(clk.rise) {
        if rst {
            count_reg = 0
        } else if enable {
            count_reg = count_reg + 1
        }
    }

    // Combinational assignments — these are continuous, not clocked.
    // They drive the output ports directly from expressions.
    // No "assign" keyword needed, no wire declaration needed.
    count = count_reg

    // overflow is high when the counter is at max AND enabled,
    // meaning it will wrap on the next clock edge.
    // This is a forward reference to nothing special — combinational
    // signals can reference each other in any order.
    overflow = (count_reg == 255) & enable
}
```

### What Is Happening Here

**`entity Counter`** declares the interface. It lists every port with a direction (`in` or `out`) and a type. There is no logic here — only the contract that this block of hardware exposes to the outside world. Ports are separated by commas.

**`impl Counter`** contains the behavior. Inside it you write signals, sequential blocks, and combinational assignments. The impl must satisfy every output port declared in the entity — if you forget to drive `overflow`, the compiler tells you.

**`signal count_reg: nat[8]`** declares internal state. This is neither a wire nor a register by declaration. It becomes a register because it is assigned inside `on(clk.rise)`. If you assigned it outside the `on` block, it would be combinational. The compiler makes the distinction based on usage, not declaration.

**`on(clk.rise)`** is the sequential block. It is equivalent to `always_ff @(posedge clk)` in SystemVerilog. Every assignment inside this block creates registered logic — the value updates on the clock edge, not continuously. You can have multiple `on` blocks in a single impl if needed.

**`count = count_reg`** and **`overflow = ...`** are combinational assignments. They sit outside any `on` block, so they are continuous — they update whenever their inputs change, like `assign` in SystemVerilog. No keyword is needed; a bare `name = expr` at the impl level is combinational.

### Types You Have Seen

| Type | Meaning |
|------|---------|
| `clock` | A clock signal. Used with `on(clk.rise)`. |
| `reset` | A reset signal. Can be used directly in `if rst`. |
| `nat[N]` | An unsigned integer that fits in N bits. Range: 0 to 2^N - 1. |
| `bit[N]` | An N-bit vector. No numeric interpretation — just bits. |

`nat[8]` and `bit[8]` are both 8 bits wide, but `nat[8]` carries the semantic meaning "this is an unsigned number" while `bit[8]` means "this is a bag of bits." Use `nat[N]` for counters, addresses, and arithmetic. Use `bit[N]` for flags, masks, and data that you shift or slice.

> **Coming from SystemVerilog?**
>
> The mapping is straightforward:
>
> | SystemVerilog | skalp | Notes |
> |---------------|-------|-------|
> | `module` | `entity` + `impl` | skalp separates interface from behavior |
> | `always_ff @(posedge clk)` | `on(clk.rise)` | Same semantics, less punctuation |
> | `logic [7:0] count` | `signal count: nat[8]` | No wire vs. reg — compiler decides |
> | `assign overflow = ...` | `overflow = ...` | No keyword needed for combinational |
> | `[7:0]` (8 bits, 0 to 7) | `nat[8]` (8-bit unsigned) | Width is the number, not max index |
> | No forward references | Forward references work | Combinational signals have no temporal order |
>
> The biggest conceptual shift: in SystemVerilog you must decide `wire` or `reg` when you declare a signal. In skalp, you declare `signal` and the compiler infers whether it is registered (assigned in `on`) or combinational (assigned outside `on`). This eliminates an entire class of declaration/usage mismatch errors.

---

## Running Project: Your First Build

The counter above is the first piece of the UART project. Every chapter adds files to the same `uart-tutorial` project you created during installation. Let us verify that it compiles and simulates.

Your project structure should look like this:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk
```

The `skalp.toml` was created by `skalp new` and contains:

```toml
[package]
name = "uart-tutorial"
version = "0.1.0"

[build]
top = "Counter"
```

Set the `top` field to `Counter` so the toolchain knows which entity is the design root.

### Building

Run the build command from the project root:

```bash
skalp build
```

If everything is correct, you will see output like:

```
   Compiling uart-tutorial v0.1.0
   Analyzing Counter
       Built Counter -> build/counter.sv
```

The compiler parses `counter.sk`, type-checks it, lowers it through HIR and MIR, and generates synthesizable SystemVerilog in the `build/` directory. You can inspect `build/counter.sv` to see what the compiler produced — it will be a straightforward `module` with `always_ff` and `assign` statements.

### Simulating

skalp includes a built-in simulator. Create a simple test stimulus by running:

```bash
skalp sim --entity Counter --cycles 300
```

This runs 300 clock cycles with default stimulus (reset asserted for the first 10 cycles, then released). The `enable` input defaults to high. You should see the counter increment from 0 to 255, overflow pulse once, then wrap to 0 and continue counting.

To see waveforms:

```bash
skalp sim --entity Counter --cycles 300 --vcd build/counter.vcd
```

This writes a VCD file you can open in GTKWave or any waveform viewer. You will see `count` ramping up, `overflow` pulsing at 255, and the wrap-around behavior.

### Common Errors

If you see `error: output port 'overflow' is never driven`, you forgot the combinational assignment for the overflow port. Every output declared in the entity must be assigned somewhere in the impl.

If you see `error: signal 'count_reg' used before declaration`, check that the `signal` line appears before the `on(clk.rise)` block. Signal declarations must precede their first use in sequential blocks. (Combinational signals can appear in any order, but signal declarations cannot.)

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Entity declaration | `entity Name { ... }` | `entity Counter { in clk: clock, out count: nat[8] }` |
| Implementation | `impl Name { ... }` | `impl Counter { ... }` |
| Input port | `in name: type` | `in enable: bit[1]` |
| Output port | `out name: type` | `out overflow: bit[1]` |
| Internal state | `signal name: type` | `signal count_reg: nat[8]` |
| Sequential logic | `on(clk.rise) { ... }` | Assignments inside become registers |
| Combinational logic | `name = expr` | `overflow = (count_reg == 255) & enable` |
| Clock type | `clock` | `in clk: clock` |
| Reset type | `reset` | `in rst: reset` |
| Unsigned integer | `nat[N]` | `nat[8]` = 8-bit unsigned (0..255) |
| Bit vector | `bit[N]` | `bit[1]` = single bit flag |
| Comment | `//` | `// this is a comment` |

---

## Next: State Machines

The counter is a single-state design — it does one thing on every clock edge. Real hardware needs to do different things depending on where it is in a sequence. In Chapter 2, you will build state machines using `if-else` chains inside `on(clk.rise)`, starting with a traffic light controller and then building the UART transmitter — the first real piece of the UART peripheral.

You will learn how to:
- Encode FSM states as integer values in a `signal`
- Use baud rate counters to time serial bit transmission
- Shift data out one bit at a time with a shift register
- Structure state transitions cleanly with nested `if-else`
- Use forward references for combinational tick signals

Continue to [Chapter 2: State Machines -- UART Transmitter](../02-state-machines/).
