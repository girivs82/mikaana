---
title: "Chapter 6: Structs and Hierarchical Composition"
date: 2025-07-15
summary: "Group related signals into structs for cleaner interfaces, then compose entities hierarchically with let-binding to build the complete UART top-level module."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 6
ShowToc: true
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

## Running Project: UART Configuration and Status Structs

Time to apply structs to the UART. Right now the transmitter and receiver each have a handful of individual control and status signals — baud divider, data bit count, busy flags, error flags. As you add more features, these multiply. Structs let you group them into logical bundles that are easier to connect and reason about.

### Defining the Structs

Create a file called `src/uart_types.sk`:

```
// uart_types.sk — Shared type definitions for the UART peripheral.
//
// These structs define the configuration and status interfaces
// used by UartTop. They are flattened to individual signals
// at synthesis time.

// UartConfig holds all runtime-configurable parameters.
// These values can be set by a bus interface or hardwired
// at the top level — the UART core does not care.
pub struct UartConfig {
    baud_divider: nat[16],    // clock cycles per bit (e.g., 434 for 115200 @ 50MHz)
    data_bits: nat[4],        // number of data bits (5, 6, 7, or 8)
    parity_enable: bit[1],    // 1 = parity bit transmitted/checked
    stop_bits: nat[2]         // 1 or 2 stop bits
}

// UartStatus aggregates all status signals from the UART.
// A bus interface can read this struct as a status register.
pub struct UartStatus {
    tx_busy: bit[1],          // TX FSM is not idle
    tx_fifo_full: bit[1],     // TX FIFO cannot accept more data
    tx_fifo_empty: bit[1],    // TX FIFO has no pending data
    rx_valid: bit[1],         // RX FIFO has data available to read
    rx_fifo_full: bit[1],     // RX FIFO is full — new data will be lost
    rx_fifo_empty: bit[1],    // RX FIFO has no data
    frame_error: bit[1],      // RX detected missing stop bit
    overrun_error: bit[1]     // RX data arrived while FIFO was full
}
```

Notice that `UartConfig` contains fields of different types — `nat[16]`, `nat[4]`, `bit[1]`, `nat[2]`. Structs are not constrained to uniform field types. Each field is independently typed and independently flattened.

### Composing the UART Top Level

Now for the hierarchical composition. `UartTop` instantiates the TX path (FIFO + transmitter) and the RX path (receiver + FIFO), wires them together, and exposes struct-based configuration and status ports.

Create a file called `src/uart_top.sk`:

```
// uart_top.sk — Top-level UART entity.
//
// Composes UartTx, UartRx, and two FIFOs into a single
// peripheral with struct-based configuration and status.
// This is the entity you instantiate in your SoC.

entity UartTop<const FIFO_DEPTH: nat = 16> {
    in clk: clock,
    in rst: reset,

    // Configuration — can be hardwired or driven by a bus
    in config: UartConfig,

    // Aggregated status — readable by a bus interface
    out status: UartStatus,

    // Write interface: host pushes data into TX FIFO
    in tx_data: bit[8],
    in tx_write: bit[1],

    // Read interface: host pulls data from RX FIFO
    out rx_data: bit[8],
    in rx_read: bit[1],

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
        clk: clk,
        rst: rst,
        write_en: tx_write,
        write_data: tx_data,
        read_en: tx_read_en
    }

    // tx_read_en is a combinational signal that pulses when the
    // transmitter is ready and the FIFO has data.
    // Forward reference: tx_read_en is used above but defined below.
    // This is legal because it is combinational.
    signal tx_ready: bit[1]
    tx_read_en = tx_ready & !tx_fifo.empty

    let uart_tx = UartTx {
        clk: clk,
        rst: rst,
        baud_divider: config.baud_divider,
        data_bits: config.data_bits,
        parity_enable: config.parity_enable,
        stop_bits: config.stop_bits,
        data_in: tx_fifo.read_data,
        data_valid: tx_read_en,
        tx: _,         // connected to top-level tx below
        busy: _        // read via uart_tx.busy
    }

    // Drive the top-level tx output from the transmitter.
    tx = uart_tx.tx

    // tx_ready: the transmitter can accept new data.
    // Assigned here — forward-referenced above. Combinational
    // signals can appear in any order.
    tx_ready = !uart_tx.busy

    // --- RX path ---
    //
    // Data flows: rx pin -> uart_rx -> rx_fifo -> rx_data

    let uart_rx = UartRx {
        clk: clk,
        rst: rst,
        baud_divider: config.baud_divider,
        data_bits: config.data_bits,
        parity_enable: config.parity_enable,
        rx: rx,
        data_out: _,       // read via uart_rx.data_out
        rx_valid: _,       // read via uart_rx.rx_valid
        frame_error: _     // read via uart_rx.frame_error
    }

    let rx_fifo = FIFO<8, FIFO_DEPTH> {
        clk: clk,
        rst: rst,
        write_en: uart_rx.rx_valid,
        write_data: uart_rx.data_out,
        read_en: rx_read
    }

    // Drive the top-level rx_data output from the RX FIFO.
    rx_data = rx_fifo.read_data

    // --- Status aggregation ---
    //
    // Construct the UartStatus struct from individual signals
    // gathered from the sub-entities. Every field must be assigned.
    //
    // The overrun_error field is combinational logic: it is high
    // when the RX FIFO is full and the receiver has new data.
    // This means the incoming byte will be dropped.

    status = UartStatus {
        tx_busy: uart_tx.busy,
        tx_fifo_full: tx_fifo.full,
        tx_fifo_empty: tx_fifo.empty,
        rx_valid: !rx_fifo.empty,
        rx_fifo_full: rx_fifo.full,
        rx_fifo_empty: rx_fifo.empty,
        frame_error: uart_rx.frame_error,
        overrun_error: rx_fifo.full & uart_rx.rx_valid
    }
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

For output ports, you have two choices:

1. **Connect to a signal.** Write `output_port: some_signal` and the output drives that signal directly.
2. **Leave unbound with `_`.** Write `output_port: _` to acknowledge the output exists but indicate that you will read it via the `instance.port` syntax instead.

The `_` is not "unconnected" in the hardware sense — the output still exists in the synthesized design. It means "I do not want to wire this to a named signal here; I will access it as `instance_name.output_port` elsewhere." This is why `uart_tx.busy` works in the status struct construction even though the `busy` port was bound to `_` in the instantiation.

### Accessing Sub-Entity Outputs

After a `let` binding, you access the outputs of the sub-entity with dot notation:

```
// These all read outputs from sub-entity instances:
tx = uart_tx.tx
tx_ready = !uart_tx.busy
status.frame_error = uart_rx.frame_error
```

This syntax works anywhere a signal reference works — in combinational assignments, in struct constructions, in `on` blocks, and in other `let` bindings. The compiler resolves `uart_tx.tx` to the physical output signal of the `UartTx` instance.

You cannot access input ports this way. `uart_tx.data_in` would be a compile error — inputs are driven by the connection you specified in the `let` binding, not read back from the instance.

### Struct Ports and Hierarchical Boundaries

Notice how `config` is a `UartConfig` struct port on `UartTop`, but `UartTx` and `UartRx` take individual parameters like `baud_divider` and `data_bits`. You bridge the gap with field access:

```
let uart_tx = UartTx {
    baud_divider: config.baud_divider,
    data_bits: config.data_bits,
    parity_enable: config.parity_enable,
    ...
}
```

The `config.baud_divider` expression extracts the `baud_divider` field from the `UartConfig` struct and passes it to the `UartTx` port. At the MIR level, this is a direct wire from the flattened `config_baud_divider` signal to the `uart_tx` instance's `baud_divider` input. No intermediate logic, no runtime cost.

You could also refactor `UartTx` to accept a `UartConfig` struct directly. Whether to pass the struct or individual fields is a design choice — structs are better when the child needs most of the fields, individual ports are better when the child only needs one or two.

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
    uart_types.sk       // Chapter 6 — UartConfig, UartStatus structs
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

You should see the compiler discover all source files, resolve the struct types, flatten them at MIR, and generate SystemVerilog for every entity:

```
   Compiling uart-tutorial v0.1.0
   Analyzing UartConfig (struct — flattened)
   Analyzing UartStatus (struct — flattened)
   Analyzing FIFO<8, 16>
   Analyzing UartTx
   Analyzing UartRx
   Analyzing UartTop<16>
       Built UartTop -> build/uart_top.sv
```

Inspect the generated `build/uart_top.sv`. You will see that the `config` port has been flattened into `config_baud_divider`, `config_data_bits`, `config_parity_enable`, and `config_stop_bits`. The `status` port is similarly flattened into eight individual output signals. The struct names do not appear anywhere in the SystemVerilog — they exist only in your skalp source.

Run a basic simulation to verify connectivity:

```bash
skalp sim --entity UartTop --cycles 5000 --vcd build/uart_top.vcd
```

Open the VCD in a waveform viewer and confirm that writing a byte to `tx_data` with `tx_write` high causes the byte to appear on the `tx` pin after FIFO and baud-rate delays. Send a serial byte on the `rx` pin and confirm it appears on `rx_data` with `rx_valid` asserted in the status output.

If you see `error: port 'stop_bits' not connected`, you forgot to wire one of the config fields to a sub-entity. The compiler enforces that every input port has a connection — there are no implicit defaults.

If you see `error: output 'frame_error' is unused`, you declared an output port with `_` but never read it via `instance.port` syntax. Either connect it to a signal in the `let` binding or reference it somewhere in the impl.

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
