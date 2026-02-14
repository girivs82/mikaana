---
title: "Chapter 4: Arrays and Generics — FIFO Buffering"
date: 2025-07-15
summary: "Array types, generic parameters, and clog2() — build a parameterized FIFO and add buffering to the UART."
tags: ["skalp", "tutorial", "fifo", "generics", "arrays"]
weight: 4
ShowToc: true
---

## What You'll Learn

Your UART transmitter and receiver from Chapters 2 and 3 work, but they have no buffering. If the TX is busy serialising a byte and you try to send another, the new byte is lost. If the RX completes a byte and the consumer is not ready, it disappears. Real peripherals solve this with **FIFOs** (first-in, first-out buffers).

Building a FIFO is also the perfect vehicle for learning three important skalp features:

- **Array types** — `[T; N]` declares a fixed-size array of elements, used here as the FIFO storage.
- **Generic parameters** — `entity Name<const PARAM: nat = default>` lets you write one module that works at any width and depth.
- **`clog2()`** — a compile-time function that computes the number of bits needed to represent a value, used for pointer and counter widths.

By the end of this chapter, your UART will have TX and RX FIFOs, cleanly decoupling the application logic from the serial timing. Your running project will include:

- `src/fifo.sk` — a standalone parameterized synchronous FIFO
- `src/uart_buffered.sk` — the UART with TX and RX FIFOs integrated

---

## Standalone Example: Parameterized FIFO

A synchronous FIFO is a circular buffer with read and write pointers. You write data at the write pointer, read data at the read pointer, and each pointer wraps around when it reaches the end of the buffer. A separate count tracks how many elements are currently stored, which determines the full and empty flags.

### The Interface

Before looking at the implementation, let us study the entity declaration:

```skalp
entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in  clk:     clock
    in  rst:     reset
    in  wr_en:   bit
    in  wr_data: bit[WIDTH]
    in  rd_en:   bit
    out rd_data: bit[WIDTH]
    out full:    bit
    out empty:   bit
    out count:   nat[clog2(DEPTH + 1)]
}
```

There is a lot packed into this declaration. Let us unpack it:

- `<const WIDTH: nat = 8, const DEPTH: nat = 16>` — these are **generic parameters**. `WIDTH` controls how many bits each entry has, `DEPTH` controls how many entries the FIFO can hold. Both have defaults (8 and 16), so you can instantiate a `FIFO` without specifying them and get an 8-bit, 16-deep buffer.
- `bit[WIDTH]` — the data ports use the generic parameter in their type. When you instantiate `FIFO<32, 64>`, `wr_data` and `rd_data` become 32-bit ports.
- `nat[clog2(DEPTH + 1)]` — the `count` output uses `clog2()` to compute its width. For a 16-deep FIFO, the count ranges from 0 to 16 (17 possible values), which requires `clog2(17) = 5` bits. For a 64-deep FIFO, it would be `clog2(65) = 7` bits. The type adapts automatically.

### The Implementation

Create `src/fifo.sk`:

```skalp
// fifo.sk — parameterized synchronous FIFO with full/empty flags

entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in  clk:     clock
    in  rst:     reset
    in  wr_en:   bit
    in  wr_data: bit[WIDTH]
    in  rd_en:   bit
    out rd_data: bit[WIDTH]
    out full:    bit
    out empty:   bit
    out count:   nat[clog2(DEPTH + 1)]
}

impl FIFO {
    // Storage array — DEPTH entries, each WIDTH bits wide.
    // This is the actual memory that holds the buffered data.
    // Depending on the synthesis target and size, the tool may
    // infer this as registers, distributed RAM, or block RAM.
    signal memory: [bit[WIDTH]; DEPTH]

    // Write and read pointers. clog2(DEPTH) gives us just enough
    // bits to index every position in the array.
    //
    // For DEPTH = 16: clog2(16) = 4, so pointers are 4 bits
    //     (values 0..15 — exactly the valid indices).
    // For DEPTH = 32: clog2(32) = 5, so pointers are 5 bits
    //     (values 0..31).
    signal wr_ptr: nat[clog2(DEPTH)]
    signal rd_ptr: nat[clog2(DEPTH)]

    // Element count. Needs clog2(DEPTH + 1) bits because the count
    // ranges from 0 (empty) to DEPTH (full). For a 16-deep FIFO,
    // that's 0..16, which requires 5 bits — one more than the
    // pointer width. This extra bit is how we distinguish "full"
    // from "empty" (both have equal pointers, but different counts).
    signal elem_count: nat[clog2(DEPTH + 1)]

    on(clk.rise) {
        if rst {
            wr_ptr     = 0
            rd_ptr     = 0
            elem_count = 0
        } else {
            // Handle simultaneous read and write first — this is
            // a common case when the FIFO is acting as a pipeline
            // stage, and handling it explicitly avoids a corner
            // case where we'd incorrectly change the count.
            if wr_en && !full_flag && rd_en && !empty_flag {
                // Simultaneous read and write — count stays the same.
                // We write a new element and read an old one in the
                // same cycle.
                memory[wr_ptr] = wr_data

                // Advance write pointer with wrap
                if wr_ptr == DEPTH - 1 {
                    wr_ptr = 0
                } else {
                    wr_ptr = wr_ptr + 1
                }

                // Advance read pointer with wrap
                if rd_ptr == DEPTH - 1 {
                    rd_ptr = 0
                } else {
                    rd_ptr = rd_ptr + 1
                }

                // elem_count does not change

            } else if wr_en && !full_flag {
                // Write only — store the data and advance the
                // write pointer.
                memory[wr_ptr] = wr_data

                if wr_ptr == DEPTH - 1 {
                    wr_ptr = 0
                } else {
                    wr_ptr = wr_ptr + 1
                }

                elem_count = elem_count + 1

            } else if rd_en && !empty_flag {
                // Read only — advance the read pointer.
                // The data is already available on rd_data
                // (driven combinationally below).
                if rd_ptr == DEPTH - 1 {
                    rd_ptr = 0
                } else {
                    rd_ptr = rd_ptr + 1
                }

                elem_count = elem_count - 1
            }
        }
    }

    // Combinational outputs — these are always valid, driven from
    // registered state. Forward references to full_flag and empty_flag
    // work because skalp resolves combinational dependencies.
    //
    // These signals are used both inside the on block (to guard
    // writes and reads) and outside (to drive the output ports).

    signal full_flag:  bit
    signal empty_flag: bit

    full_flag  = elem_count == DEPTH
    empty_flag = elem_count == 0

    // Output ports
    full    = full_flag
    empty   = empty_flag
    count   = elem_count
    rd_data = memory[rd_ptr]
}
```

### Anatomy of the FIFO

Let us examine each key feature in detail.

**Generic parameters.** The `<const WIDTH: nat = 8, const DEPTH: nat = 16>` syntax declares two compile-time constants with defaults. When you instantiate the FIFO, you can override either or both. The compiler substitutes the values before synthesis, producing specialised hardware — there is no runtime cost. A `FIFO<8, 16>` produces a design with an 8-bit-wide, 16-deep memory. A `FIFO<32, 256>` produces a completely different design with a 32-bit-wide, 256-deep memory and wider pointers.

Generic parameters in skalp are true type-level constants. They can appear in type expressions (`bit[WIDTH]`), in `clog2()` arguments, in comparison values (`DEPTH - 1`), and anywhere else a compile-time constant is valid. The compiler evaluates all generic expressions at compile time and checks that the resulting types are consistent.

**Array type.** `[bit[WIDTH]; DEPTH]` is an array of `DEPTH` elements, each of type `bit[WIDTH]`. The syntax reads as "an array of DEPTH things of type bit[WIDTH]." This maps to a register file or block RAM depending on the synthesis target and the array size.

You index into the array with `memory[wr_ptr]`, and you assign individual elements with `memory[wr_ptr] = wr_data`. The index must be of a type wide enough to cover all valid indices — here, `nat[clog2(DEPTH)]` guarantees this.

Arrays in skalp are fixed-size. The size is a compile-time constant (it can use generic parameters and `clog2()`). There are no dynamically-sized arrays — this is hardware, and the size of every memory must be known at synthesis time.

**`clog2()`.** This function computes the ceiling of log-base-2 at compile time. Some examples:

| Expression | Value | Why |
|---|---|---|
| `clog2(16)` | 4 | 2^4 = 16, so 4 bits cover indices 0..15 |
| `clog2(17)` | 5 | 2^4 = 16 < 17, so we need 5 bits for 0..16 |
| `clog2(1)` | 0 | A single-element "array" needs 0 index bits |
| `clog2(DEPTH)` | varies | Adapts to the generic parameter |
| `clog2(DEPTH + 1)` | varies | One extra bit for the count (0 to DEPTH inclusive) |

The result of `clog2()` is used directly in type declarations like `nat[clog2(DEPTH)]` — the pointer width adapts automatically to the FIFO depth. You never need to manually compute the width or define a separate constant for it.

**Pointer wrapping.** Instead of relying on power-of-two depths and bitmask tricks (a common SV pattern), this FIFO uses explicit comparison: `if wr_ptr == DEPTH - 1 { wr_ptr = 0 } else { wr_ptr = wr_ptr + 1 }`. This works for any depth, not just powers of two — you can have a 17-deep or 100-deep FIFO.

For power-of-two depths, the synthesiser recognises this pattern and optimises it to a simple bit truncation, producing the same hardware as the bitmask approach. For non-power-of-two depths, it generates a comparator and mux, which is exactly what you need.

**Forward references.** Notice that `full_flag` and `empty_flag` are used inside the `on` block (in the guard conditions `!full_flag` and `!empty_flag`) but defined and assigned *below* the `on` block. This is legal in skalp: combinational signals can be referenced before their assignment. The compiler builds a dependency graph and resolves the order.

This is one of skalp's most convenient features for code organisation. You can group all the sequential logic in one block and all the combinational outputs below it, without worrying about declaration order. The compiler ensures there are no true combinational loops (those are errors), but forward references to signals that are eventually assigned are perfectly fine.

**Simultaneous read and write.** The FIFO explicitly handles the case where both `wr_en` and `rd_en` are asserted in the same cycle. This is important: if the FIFO is used as a pipeline stage, the upstream may be writing while the downstream is reading. Without explicit handling, the count would incorrectly increment (from the write) and decrement (from the read) in the same cycle. By checking for the simultaneous case first, we keep the count unchanged and simply advance both pointers.

---

> **Coming from SystemVerilog?**
>
> The FIFO maps closely to what you would write in SV, but there are a few notable differences:
>
> - **Array syntax is inverted.** In skalp, `[bit[8]; 16]` means "16 elements of 8-bit vectors." In SV, the equivalent is `logic [7:0] mem [0:15]` — type first with the bit range, then the array dimension. Skalp puts the element type inside the brackets and the count outside, which reads more naturally as "an array of N things of type T." If you have used Rust, this syntax will feel familiar.
> - **`clog2()` vs `$clog2()`.** Both compute the same value, but skalp's `clog2()` is a compile-time type-level function. You use it directly in type declarations: `nat[clog2(DEPTH)]`. There is no `$` prefix and no need for a separate `localparam` to hold the result. The compiler evaluates it during type checking, not during elaboration.
> - **No `[WIDTH-1:0]` pattern.** In SV, `logic [7:0]` means bits 7 down to 0 — an 8-bit vector. In skalp, `bit[8]` means an 8-bit vector. The width is the natural number, not a range. You never write `-1` in a type declaration. This eliminates a common source of off-by-one bugs: `logic [WIDTH:0]` vs `logic [WIDTH-1:0]` — in skalp, you just write `bit[WIDTH]` or `bit[WIDTH + 1]`.
> - **Generic params vs `parameter`.** SV uses `parameter` inside the module or `#(parameter ...)` in the port list. Skalp uses `<const NAME: type = default>` on the entity declaration. The semantics are the same — compile-time constants that specialise the module — but skalp's generics are part of the type system, so a `FIFO<8, 16>` and a `FIFO<32, 64>` are distinct types. You cannot accidentally connect a 32-bit FIFO's output to an 8-bit consumer without the compiler catching it.
> - **No sensitivity lists.** In SV, `always_comb` requires the tool to infer the sensitivity list, and incomplete lists are a common bug source. In skalp, combinational assignments are just expressions outside `on` blocks — the compiler knows every dependency.

---

## Running Project: Adding FIFOs to the UART

With the FIFO module ready, we can buffer both the TX and RX paths. The updated architecture looks like this:

```
                  ┌──────────┐     ┌──────────┐
  tx_data_in ──>  │  TX FIFO │ ──> │  UartTx  │ ──> tx (pin)
  tx_write   ──>  │          │     │          │
                  └──────────┘     └──────────┘

                  ┌──────────┐     ┌──────────┐
  rx_data_out <── │  RX FIFO │ <── │  UartRx  │ <── rx (pin)
  rx_read     ──> │          │     │          │
                  └──────────┘     └──────────┘
```

The application writes bytes into the TX FIFO. When the FIFO is not empty and the transmitter is idle, the UART controller pops a byte and feeds it to the transmitter. On the receive side, when the receiver completes a byte, it is pushed into the RX FIFO. The application reads bytes out of the RX FIFO at its own pace.

### The Buffered UART

Create `src/uart_buffered.sk`:

```skalp
// uart_buffered.sk — UART with TX and RX FIFOs

entity UartBuffered {
    in  clk:          clock
    in  rst:          reset

    // TX interface — application writes bytes here
    in  tx_data_in:   bit[8]
    in  tx_write:     bit
    out tx_full:      bit

    // RX interface — application reads bytes here
    out rx_data_out:  bit[8]
    in  rx_read:      bit
    out rx_empty:     bit

    // Serial pins
    out tx:           bit
    in  rx:           bit

    // Status
    out tx_count:     nat[5]
    out rx_count:     nat[5]
}

impl UartBuffered {
    // Internal signals for FIFO-to-UART wiring.
    // These connect the FIFO outputs to the UART inputs and vice versa.
    signal tx_fifo_data:  bit[8]
    signal tx_fifo_empty: bit
    signal tx_fifo_read:  bit
    signal tx_busy:       bit

    signal rx_byte:       bit[8]
    signal rx_valid:      bit
    signal rx_fifo_full:  bit

    // TX FIFO: 8 bits wide, 16 entries deep.
    // The application writes into this FIFO, and the UART TX
    // reads from it automatically.
    let tx_fifo = FIFO<8, 16> {
        clk:     clk,
        rst:     rst,
        wr_en:   tx_write,
        wr_data: tx_data_in,
        rd_en:   tx_fifo_read,
        rd_data: tx_fifo_data,
        full:    tx_full,
        empty:   tx_fifo_empty,
        count:   tx_count
    }

    // RX FIFO: 8 bits wide, 16 entries deep.
    // The UART RX writes into this FIFO when a byte is received,
    // and the application reads from it at its leisure.
    let rx_fifo = FIFO<8, 16> {
        clk:     clk,
        rst:     rst,
        wr_en:   rx_valid && !rx_fifo_full,
        wr_data: rx_byte,
        rd_en:   rx_read,
        rd_data: rx_data_out,
        full:    rx_fifo_full,
        empty:   rx_empty,
        count:   rx_count
    }

    // UART transmitter — reads from the TX FIFO.
    // tx_fifo_read serves as both the FIFO read enable and the
    // UART transmit enable — when we pop a byte from the FIFO,
    // we simultaneously start transmitting it.
    let uart_tx = UartTx {
        clk:     clk,
        rst:     rst,
        tx_data: tx_fifo_data,
        tx_en:   tx_fifo_read,
        tx:      tx,
        tx_busy: tx_busy
    }

    // UART receiver — writes to the RX FIFO.
    // When rx_valid pulses, the byte on rx_byte is captured by
    // the RX FIFO (if not full).
    let uart_rx = UartRx {
        clk:      clk,
        rst:      rst,
        rx:       rx,
        rx_data:  rx_byte,
        rx_valid: rx_valid
    }

    // TX FIFO read controller: pop a byte when the transmitter
    // is idle and the FIFO has data. This single combinational
    // expression is all the "glue logic" needed to connect the
    // FIFO to the transmitter.
    tx_fifo_read = !tx_fifo_empty && !tx_busy
}
```

### How the Pieces Fit Together

**TX path.** The application asserts `tx_write` with `tx_data_in` to push a byte into the TX FIFO. The combinational expression `tx_fifo_read = !tx_fifo_empty && !tx_busy` automatically triggers a read from the FIFO whenever the transmitter is idle and data is available. The FIFO's `rd_data` output feeds directly into `UartTx`'s `tx_data` port, and `tx_fifo_read` serves double duty as the transmitter's `tx_en` — starting transmission of the byte in the same cycle it is read from the FIFO.

This means bytes flow through the system automatically: the application pushes them into the FIFO, and the transmitter pulls them out one at a time as fast as the baud rate allows. The application never needs to check whether the transmitter is busy — it just writes to the FIFO and checks `tx_full` to avoid overflow.

**RX path.** When `UartRx` completes a byte, it pulses `rx_valid` for one cycle (as we designed in Chapter 3). The RX FIFO's `wr_en` is `rx_valid && !rx_fifo_full` — it captures the byte unless the FIFO is full. If the FIFO *is* full when a byte arrives, the byte is dropped. A production design might set an overflow flag here.

The application reads bytes out by asserting `rx_read`, and checks `rx_empty` to know whether data is available. The FIFO's `rd_data` output always holds the oldest unread byte.

**Status signals.** The `tx_count` and `rx_count` outputs expose how many bytes are in each FIFO. These are useful for flow control (e.g., stop accepting data when the TX FIFO is more than 75% full) or for diagnostics (a debugger can read the FIFO depth to understand system behaviour).

**The glue line.** The most important line in the entire module is the last one:

```skalp
tx_fifo_read = !tx_fifo_empty && !tx_busy
```

This single combinational expression is the "controller" that connects the FIFO to the transmitter. It says: "whenever there is data to send and the transmitter is not busy, start sending." No state machine, no additional logic — just a Boolean expression. This is the power of breaking a design into well-defined sub-modules with clean interfaces.

### Generic Instantiation Syntax

The instantiation `FIFO<8, 16>` passes generic arguments by position, matching `WIDTH = 8` and `DEPTH = 16`. You can also use the defaults:

```skalp
// Uses WIDTH = 8, DEPTH = 16 (both defaults)
let default_fifo = FIFO { clk: clk, rst: rst, ... }

// Override only DEPTH, WIDTH uses default of 8
let deep_fifo = FIFO<8, 256> { clk: clk, rst: rst, ... }

// 32-bit wide, 64 entries deep
let wide_fifo = FIFO<32, 64> { clk: clk, rst: rst, ... }
```

Each instantiation produces distinct hardware. `FIFO<8, 16>` and `FIFO<32, 64>` are completely independent — different memory sizes, different pointer widths, different count widths. The compiler computes `clog2()` for each set of parameters and generates the appropriate bit widths. At synthesis time, there is no "generic" FIFO — there are only the concrete, specialised versions.

### Why Count-Based, Not Pointer-Difference?

You might wonder why the FIFO uses a separate `elem_count` register instead of deriving fullness from the pointer difference (`wr_ptr - rd_ptr`). Both approaches work, but they have different trade-offs:

**Count-based** (our approach):
- Extra register (`elem_count`), costs `clog2(DEPTH + 1)` flip-flops
- Full and empty are simple comparisons: `count == DEPTH`, `count == 0`
- Works for any DEPTH, not just powers of two
- The count output is directly available — no additional logic

**Pointer-difference:**
- No extra register — derive count from `wr_ptr - rd_ptr`
- Requires power-of-two DEPTH for the subtraction to wrap correctly (or an extra MSB on each pointer)
- Full vs empty ambiguity when pointers are equal: is it 0 elements or DEPTH elements?
- Common SV solution: add one extra bit to each pointer, compare the MSB to distinguish full from empty

Both are valid. The count-based approach is simpler to understand and works for arbitrary depths, which is why we use it here. A power-of-two optimised FIFO might use the pointer-difference approach to save a few flip-flops.

---

## Build and Test

Compile the FIFO standalone:

```bash
skalp build src/fifo.sk
```

Compile the buffered UART (pulls in the FIFO, UartTx, and UartRx):

```bash
skalp build src/uart_buffered.sk
```

Run the test suite:

```bash
skalp test src/uart_buffered.sk --trace
```

Verify in the waveform viewer:

1. Write several bytes in quick succession to `tx_data_in` — confirm they are buffered in the TX FIFO and transmitted one at a time.
2. Watch `tx_count` decrement as each byte finishes transmitting.
3. Confirm that `tx_fifo_read` pulses high at the exact moment between transmissions — when the transmitter finishes one byte and the FIFO still has data.
4. Send multiple bytes on the `rx` line — confirm they appear in the RX FIFO and `rx_count` increments.
5. Read bytes from the RX FIFO with `rx_read` — confirm `rx_data_out` produces them in FIFO order (first written, first read) and `rx_empty` goes high when the last byte is consumed.
6. Try to overfill the TX FIFO — write 17 or more bytes and confirm `tx_full` goes high after 16 writes and subsequent writes are ignored (the count stays at 16).
7. Test simultaneous read and write: while the TX is busy transmitting and the FIFO has some entries, write another byte. Confirm the count changes correctly.

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Array type | `[T; N]` | `signal memory: [bit[8]; 16]` |
| Array indexing | `array[index]` | `memory[wr_ptr]` |
| Array element write | `array[index] = value` | `memory[wr_ptr] = wr_data` |
| Generic entity | `entity Name<const P: type = default>` | `entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16>` |
| Generic instantiation | `Entity<args>` | `FIFO<8, 16> { ... }` |
| Compile-time log2 | `clog2(expr)` | `nat[clog2(DEPTH)]` |
| Pointer width from depth | `nat[clog2(N)]` | `signal wr_ptr: nat[clog2(DEPTH)]` |
| Count width (extra bit) | `nat[clog2(N + 1)]` | `signal elem_count: nat[clog2(DEPTH + 1)]` |
| Forward reference | Use before assign (combinational) | `full_flag` used in `on`, assigned below |
| Default generic values | Omit args to use defaults | `FIFO { ... }` uses WIDTH=8, DEPTH=16 |

---

## Next

Your UART now has buffering, but both the transmitter and receiver have hardcoded parameters: 50 MHz clock, 115200 baud, 8 data bits. If you want to target a different FPGA or baud rate, you need to edit the source. The FIFO depth is also hardcoded at 16 — what if you need deeper buffers for a high-throughput application, or shallower ones to save resources?

Right now, changing any of these parameters means editing multiple files and recalculating constants by hand. There is a better way.

In **[Chapter 5: Const Generics and Parameterization](../05-parameterization/)**, you will make the entire UART configurable — clock frequency, baud rate, FIFO depth — using generic parameters and compile-time computation. You will learn how constants like `CYCLES_PER_BIT` can be computed from higher-level parameters like `CLK_FREQ` and `BAUD_RATE`, so you specify the intent and let the compiler do the arithmetic. You will also learn how to use different parameter sets for simulation versus synthesis, avoiding the pain of waiting for real-time baud periods in test.
