---
title: "Chapter 6: Structs and Hierarchical Composition"
date: 2025-07-15
summary: "Group related signals into structs for cleaner interfaces, then compose entities hierarchically with let-binding to build the complete UART top-level module."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 6
ShowToc: true
aliases: ["/tutorial/06-structs-and-composition/"]
---

## What This Chapter Teaches

Over the last five chapters you have built individual UART components — a transmitter, a receiver, parameterized FIFOs, and a fully configurable parameter set. Each component works, but connecting them means threading a dozen individual signals between entities. That is tedious, error-prone, and hard to read.

skalp **structs** solve this. A struct groups related signals into a single named type that you can use as a port, pass between entities, and construct with named fields. At the language level structs give you structure and type safety. At the synthesis level they disappear entirely — the compiler flattens every struct into individual signals before generating SystemVerilog. You get the ergonomics of grouping without any synthesis cost.

This chapter also introduces **hierarchical composition** with `let` bindings. Instead of wiring entities together with raw signal names, you instantiate sub-entities inside an impl and access their outputs by name. This is how you assemble a complete design from tested components.

By the end of this chapter you will understand:

- How to define structs with `pub struct`
- How to use structs as port types in entity declarations
- How to construct struct values with named fields
- How to access individual fields with dot notation
- How struct flattening works at MIR — what synthesis actually sees
- How to nest structs inside other structs
- How to instantiate sub-entities with `let` bindings
- How to access sub-entity outputs with `instance.port` syntax
- That all ports must be connected — no implicit defaults, no positional binding

These concepts come together in the running project, where you build `UartTop` — a single entity that wires the transmitter, receiver, and FIFOs together behind clean struct-based configuration and status ports.

---

## Standalone Example: RGB Color Mixer

Let us start with a small example that shows struct definition, struct ports, field access, and struct construction in one place. We will build a color mixer that blends two RGB colors based on a mix factor.

Create a file called `src/color_mixer.sk`:

```
// RGB Color Mixer
//
// Blends two colors using a linear interpolation factor.
// mix_factor = 0   -> output is entirely color_a
// mix_factor = 255 -> output is entirely color_b
// Values in between produce a proportional blend.

// A struct groups related signals under a single name.
// "pub" makes the struct visible to other files that import this module.
// Without pub, the struct is private to this file.
pub struct Color {
    r: bit[8],
    g: bit[8],
    b: bit[8]
}

// Structs can be used directly as port types.
// The compiler knows the width of a Color (24 bits total)
// but treats each field as a separate signal internally.
entity ColorMixer {
    in clk: clock,
    in rst: reset,
    in color_a: Color,
    in color_b: Color,
    in mix_factor: nat[8],
    out result: Color
}

impl ColorMixer {
    // Construct a struct value with named fields.
    // Every field must be assigned — the compiler rejects partial construction.
    //
    // Field access uses dot notation: color_a.r gives the red channel
    // of the input color_a port.
    result = Color {
        r: (color_a.r * (255 - mix_factor) + color_b.r * mix_factor) >> 8,
        g: (color_a.g * (255 - mix_factor) + color_b.g * mix_factor) >> 8,
        b: (color_a.b * (255 - mix_factor) + color_b.b * mix_factor) >> 8
    }
}
```

### How Structs Work

**Definition.** `pub struct Color { r: bit[8], g: bit[8], b: bit[8] }` declares a struct type with three fields. The `pub` keyword makes it available to other modules. Fields are separated by commas. Each field has a name and a type — the same types you use for ports and signals.

**Port usage.** In the entity declaration, `in color_a: Color` declares a port whose type is the `Color` struct. From the outside, this is a single connection point. From the inside, you access individual fields with `color_a.r`, `color_a.g`, `color_a.b`.

**Construction.** `Color { r: expr, g: expr, b: expr }` creates a struct value. Every field must be present. The order of fields does not matter — names make it unambiguous. You cannot leave a field out and you cannot add fields that do not exist in the definition.

**Field access.** Dot notation works everywhere: in expressions, in assignments, in `on` blocks, and in port connections. `color_a.r` is the red channel of the `color_a` input. `result.g` is the green channel of the `result` output. You can read and write individual fields independently.

### What Synthesis Sees

Here is the critical concept: **structs are a compile-time grouping mechanism. They do not exist in the generated SystemVerilog.** When the compiler lowers the design through MIR (Mid-level Intermediate Representation), it flattens every struct into individual signals. The synthesized output for the `ColorMixer` entity would have ports like:

```
// What the skalp compiler generates (simplified):
module ColorMixer (
    input  wire        clk,
    input  wire        rst,
    input  wire [7:0]  color_a_r,
    input  wire [7:0]  color_a_g,
    input  wire [7:0]  color_a_b,
    input  wire [7:0]  color_b_r,
    input  wire [7:0]  color_b_g,
    input  wire [7:0]  color_b_b,
    input  wire [7:0]  mix_factor,
    output wire [7:0]  result_r,
    output wire [7:0]  result_g,
    output wire [7:0]  result_b
);
```

The struct field names become part of the flattened signal name, joined by underscores. This flattening is deterministic and predictable — you can always reason about the generated names from the struct definition.

### Nested Structs

Structs can contain other structs. The flattening follows through each level:

```
pub struct Pixel {
    color: Color,
    alpha: bit[8]
}

entity Compositor {
    in clk: clock,
    in rst: reset,
    in foreground: Pixel,
    in background: Pixel,
    out blended: Pixel
}
```

The `foreground` port flattens to `foreground_color_r`, `foreground_color_g`, `foreground_color_b`, and `foreground_alpha`. Nesting depth is unlimited, but in practice two or three levels is the useful range. Beyond that, the flattened names become unwieldy and the hardware benefit diminishes.

Accessing nested fields chains the dots: `foreground.color.r` gives the red channel of the foreground pixel.

> **Coming from SystemVerilog?**
>
> SystemVerilog has `struct` types too, but they come with significant practical limitations:
>
> | SystemVerilog | skalp | Notes |
> |---------------|-------|-------|
> | `typedef struct packed` | `pub struct` | skalp structs are always synthesizable |
> | Struct ports work in some tools, break in others | Struct ports always work | skalp flattens before synthesis — tools never see structs |
> | Positional or named port connections | Named connections only | `let x = Foo { port: val }` — never positional |
> | Unconnected ports silently default to 0 | All ports must be connected | Unconnected output requires explicit `_` |
> | `.port(signal)` instantiation | `port: signal` instantiation | No dot-prefix shorthand in skalp |
>
> The biggest win: because skalp flattens structs before emitting SystemVerilog, you never hit the tool compatibility issues that plague SV struct ports. Every synthesis tool, formal tool, and linter sees plain `wire` and `logic` signals. The struct grouping lives only in your source code, where it helps you think clearly.

---

## Running Project: UART Top-Level Composition

Time to bring all the pieces together. `UartTop` instantiates the TX path (FIFO + transmitter) and the RX path (receiver + FIFO), wires them together, and exposes simple status ports. This is the entity you would instantiate in an SoC.

While a production design might group the status signals into a `UartStatus` struct (as the ColorMixer example demonstrates for the `Color` type), we keep the top-level entity simple here with individual ports. The struct-based approach would work identically — structs are purely a source-level grouping mechanism that the compiler flattens to individual signals.

### Composing the UART Top Level

Create a file called `src/uart_top.sk`:

```skalp
// uart_top.sk — Top-level UART entity.
//
// Composes UartTx, UartRx, and two FIFOs into a single
// peripheral. This is the entity you instantiate in your SoC.
//
// Uses the simple 8N1 (115200 baud) transmitter and receiver
// from earlier chapters. For a configurable version, see
// uart_top_parameterized.sk.

use fifo::FIFO;
use uart_tx::UartTx;
use uart_rx::UartRx;

entity UartTop<const FIFO_DEPTH: nat = 16> {
    in clk: clock,
    in rst: reset,

    // Write interface: host pushes data into TX FIFO
    in tx_data: bit[8],
    in tx_write: bit[1],

    // Read interface: host pulls data from RX FIFO
    out rx_data: bit[8],
    in rx_read: bit[1],

    // Status
    out tx_busy: bit[1],
    out tx_fifo_full: bit[1],
    out tx_fifo_empty: bit[1],
    out rx_fifo_full: bit[1],
    out rx_fifo_empty: bit[1],

    // Physical UART pins
    out tx: bit[1],
    in rx: bit[1]
}

impl UartTop {
    // --- TX path ---
    //
    // Data flows: tx_data -> tx_fifo -> uart_tx -> tx pin
    //
    // The "let" keyword instantiates a sub-entity and binds it
    // to a name. Every input port must be connected by name.
    // There is no positional port binding — names are mandatory.

    let tx_fifo = FIFO<8, FIFO_DEPTH> {
        clk:     clk,
        rst:     rst,
        wr_en:   tx_write,
        wr_data: tx_data,
        rd_en:   tx_read_en
    }

    // tx_read_en is a combinational signal that pulses when the
    // transmitter is ready and the FIFO has data.
    // Forward reference: tx_read_en is used above but defined below.
    // This is legal because it is combinational.
    signal tx_ready: bit[1]
    tx_read_en = tx_ready & !tx_fifo.empty

    let uart_tx_inst = UartTx {
        clk:      clk,
        rst:      rst,
        tx_data:  tx_fifo.rd_data,
        tx_start: tx_read_en
    }

    // Drive the top-level tx output from the transmitter.
    tx = uart_tx_inst.tx

    // tx_ready: the transmitter can accept new data.
    // Assigned here — forward-referenced above. Combinational
    // signals can appear in any order.
    tx_ready = !uart_tx_inst.tx_busy
    tx_busy = uart_tx_inst.tx_busy

    // --- RX path ---
    //
    // Data flows: rx pin -> uart_rx -> rx_fifo -> rx_data

    let uart_rx_inst = UartRx {
        clk: clk,
        rst: rst,
        rx:  rx
    }

    let rx_fifo = FIFO<8, FIFO_DEPTH> {
        clk:     clk,
        rst:     rst,
        wr_en:   uart_rx_inst.rx_valid,
        wr_data: uart_rx_inst.rx_data,
        rd_en:   rx_read
    }

    // Drive the top-level rx_data output from the RX FIFO.
    rx_data = rx_fifo.rd_data

    // --- Status outputs ---
    tx_fifo_full  = tx_fifo.full
    tx_fifo_empty = tx_fifo.empty
    rx_fifo_full  = rx_fifo.full
    rx_fifo_empty = rx_fifo.empty
}
```

### Understanding the `let` Binding

The `let` keyword creates a sub-entity instance. The syntax is:

```
let instance_name = EntityName<GENERIC_ARGS> {
    port_name: expression,
    port_name: expression,
    ...
}
```

Every input port must be connected. There are no positional connections — you always write `port_name: value`. This makes instantiation self-documenting and immune to port-order changes in the child entity.

Output ports are accessed after the binding with dot notation: `instance_name.output_port`. This is how you wire sub-entity outputs to signals, ports, or other sub-entity inputs.

### Accessing Sub-Entity Outputs

After a `let` binding, you access the outputs of the sub-entity with dot notation:

```skalp
// These all read outputs from sub-entity instances:
tx = uart_tx_inst.tx
tx_ready = !uart_tx_inst.tx_busy
rx_data = rx_fifo.rd_data
```

This syntax works anywhere a signal reference works — in combinational assignments, in `on` blocks, and in other `let` bindings. The compiler resolves `uart_tx_inst.tx` to the physical output signal of the `UartTx` instance.

You cannot access input ports this way. `uart_tx_inst.tx_data` would be a compile error — inputs are driven by the connection you specified in the `let` binding, not read back from the instance.

### Struct Ports and Hierarchical Boundaries

In a production design, you might group the status signals into a struct:

```skalp
pub struct UartStatus {
    tx_busy: bit[1],
    tx_fifo_full: bit[1],
    tx_fifo_empty: bit[1],
    rx_fifo_full: bit[1],
    rx_fifo_empty: bit[1]
}
```

And expose it as `out status: UartStatus` on the entity. The compiler would flatten this to `status_tx_busy`, `status_tx_fifo_full`, etc. in the generated SystemVerilog. Whether to use a struct or individual ports is a design choice — structs are better when you have many related signals that are always used together, individual ports are better for simple interfaces.

### Project Structure After This Chapter

Your project now looks like this:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk          // Chapter 1 — 8-bit counter
    uart_tx.sk          // Chapter 2 — UART transmitter
    uart_rx.sk          // Chapter 3 — UART receiver
    fifo.sk             // Chapter 4 — parameterized FIFO
    uart_buffered.sk    // Chapter 4 — UART with FIFOs
    uart_top.sk         // Chapter 6 — top-level composition
```

---

## Build and Test

Update `skalp.toml` to point at the new top-level entity:

```toml
[package]
name = "uart-tutorial"
version = "0.1.0"

[build]
top = "UartTop"
```

Build the project:

```bash
skalp build
```

You should see the compiler discover all source files, resolve the imports, and generate SystemVerilog for every entity:

```
   Compiling uart-tutorial v0.1.0
   Analyzing FIFO<8, 16>
   Analyzing UartTx
   Analyzing UartRx
   Analyzing UartTop<16>
       Built UartTop -> build/uart_top.sv
```

Inspect the generated `build/uart_top.sv`. You will see a standard SystemVerilog module with the individual ports: `tx_data`, `tx_write`, `rx_data`, `rx_read`, the status outputs, and the physical UART pins. The sub-entity instances are flattened — their internal signals become prefixed signals in the parent.

Run a basic simulation to verify connectivity:

```bash
skalp sim --entity UartTop --cycles 5000 --vcd build/uart_top.vcd
```

Open the VCD in a waveform viewer and confirm that writing a byte to `tx_data` with `tx_write` high causes the byte to appear on the `tx` pin after FIFO and baud-rate delays. Send a serial byte on the `rx` pin and confirm it appears on `rx_data` after the RX FSM completes.

If you see `error: port 'tx_start' not connected`, you forgot to wire one of the sub-entity's input ports. The compiler enforces that every input port has a connection — there are no implicit defaults.

---

## Testing Your Design

When an entity has struct-typed ports, the test API *flattens* them. A port `in color_a: Color` becomes individual signals: `color_a_r`, `color_a_g`, `color_a_b`. The naming convention is `portname_fieldname`.

Here are tests from `tests/ch06_test.rs`:

### ColorMixer

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_color_mixer_all_a() {
    let mut tb = Testbench::with_top_module("src/color_mixer.sk", "ColorMixer")
        .await.unwrap();
    tb.reset(2).await;

    // mix_factor = 0 -> output entirely color_a
    tb.set("color_a_r", 255u32);
    tb.set("color_a_g", 128u32);
    tb.set("color_a_b", 64u32);
    tb.set("color_b_r", 0u32);
    tb.set("color_b_g", 0u32);
    tb.set("color_b_b", 0u32);
    tb.set("mix_factor", 0u32);
    tb.clock(1).await;

    // Integer arithmetic loses a bit of precision:
    // (255 * 255) >> 8 = 254
    let r = tb.get_u64("result_r").await;
    assert!(r >= 253, "Red channel should be ~255, got {}", r);
}

#[tokio::test]
async fn test_color_mixer_midpoint() {
    let mut tb = Testbench::with_top_module("src/color_mixer.sk", "ColorMixer")
        .await.unwrap();
    tb.reset(2).await;

    tb.set("color_a_r", 200u32);
    tb.set("color_a_g", 0u32);
    tb.set("color_a_b", 0u32);
    tb.set("color_b_r", 0u32);
    tb.set("color_b_g", 200u32);
    tb.set("color_b_b", 0u32);
    tb.set("mix_factor", 128u32); // 50/50 blend
    tb.clock(1).await;

    let r = tb.get_u64("result_r").await;
    let g = tb.get_u64("result_g").await;
    assert!(r >= 95 && r <= 105, "Red ~100, got {}", r);
    assert!(g >= 95 && g <= 105, "Green ~100, got {}", g);
}
```

### UartTop (status ports)

```rust
#[tokio::test]
async fn test_uart_top_idle_state() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop")
        .await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // After reset, TX should be idle (high) and FIFOs should be empty
    tb.expect("tx", 1u32).await;
    tb.expect("tx_fifo_full", 0u32).await;
    tb.expect("tx_fifo_empty", 1u32).await;
    tb.expect("rx_fifo_empty", 1u32).await;

    // Run for a while with no activity — nothing should change
    tb.clock(1000).await;
    tb.expect("tx", 1u32).await;
}
```

Run with:

```bash
cargo test
```

**Exercise:** Write a `test_color_mixer_all_b` test that sets `mix_factor` to 255 and verifies the output matches `color_b`.

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Struct definition | `pub struct Name { field: type, ... }` | `pub struct Color { r: bit[8], g: bit[8], b: bit[8] }` |
| Struct port | `in name: StructType` | `in config: UartConfig` |
| Struct construction | `Name { field: expr, ... }` | `Color { r: 255, g: 0, b: 128 }` |
| Field access | `value.field` | `config.baud_divider` |
| Nested field access | `value.field.subfield` | `pixel.color.r` |
| Sub-entity instantiation | `let name = Entity { ... }` | `let tx_fifo = FIFO<8, 16> { clk: clk, ... }` |
| Generic instantiation | `Entity<ARGS> { ... }` | `FIFO<8, FIFO_DEPTH> { ... }` |
| Output access | `instance.port` | `uart_tx.busy` |
| Unbound output | `port: _` | `busy: _` (read later as `instance.busy`) |
| All ports required | — | Compiler error if any port is missing |
| Named connections only | — | No positional port binding |
| Struct flattening | Automatic at MIR | `config.baud_divider` becomes `config_baud_divider` in SV |

---

## Next: Enums and Pattern Matching

The UART transmitter and receiver use integer constants for FSM states — `0` for IDLE, `1` for START, `2` for DATA, `3` for STOP. That works, but it means a typo like `state = 5` compiles without complaint and produces silent misbehavior. Enums fix this by giving each state a name and letting the compiler enforce exhaustive handling.

In Chapter 7, you will learn:

- How to define enum types with named variants
- How `match` expressions replace `if-else` chains for state dispatch
- How the compiler checks that every variant is handled — no missing cases
- How to refactor the UART TX and RX state machines to use enums
- How enums integrate with structs for rich, self-documenting interfaces
- How to build a simple UART command parser using enum-based protocol states

Continue to [Chapter 7: Enums and Pattern Matching](../07-enums-and-matching/).
