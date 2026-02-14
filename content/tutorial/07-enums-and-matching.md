---
title: "Chapter 7: Enums and Pattern Matching"
date: 2025-07-15
summary: "Enum types with explicit bit encoding, match expressions that return values, exhaustiveness checking that catches missing cases at compile time — refactor FSM states and build a UART command parser."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 7
ShowToc: true
---

## What This Chapter Teaches

Up to now, your state machines have used bare integer constants for states: `signal IDLE: nat[2] = 0`, `signal START: nat[2] = 1`, and so on. This works, but it has a problem you have already felt: nothing in the language connects the constant `0` to the concept "idle." If you mistype a value, use the wrong width, or forget to handle a state in a transition table, the compiler does not help you. The bug shows up in simulation — or worse, on silicon.

skalp has **enum types** that solve this. An enum declares a closed set of named variants with explicit bit encodings. Combined with **`match` expressions**, you get compile-time guarantees that every variant is handled everywhere it is used. Add a new state to your FSM and forget to update the transition logic? The compiler refuses to build.

By the end of this chapter you will understand:

- How `pub enum` declares a set of named variants with explicit bit encodings
- That enums are types — you can use them for ports, signals, and struct fields
- How `match` is an **expression** that returns a value, not a statement
- That every arm of a `match` must produce the same type
- That the compiler checks **exhaustiveness** — every variant must be covered
- How `_` acts as a catch-all for values outside the named variants
- How to refactor integer-encoded FSMs into enum-driven state machines
- How to decode incoming data into command enums with `match`

These tools eliminate an entire class of state machine bugs that haunt SystemVerilog designs.

---

## Standalone Example: ALU with Operation Enum

Let us build a small ALU that takes two 8-bit operands and an operation selector. Instead of encoding operations as raw bit patterns and hoping every consumer agrees on the mapping, we define an enum.

Create a file called `src/alu.sk`:

```
// alu.sk — 8-bit ALU with enum-selected operations

// The operation enum. Each variant has an explicit 3-bit encoding.
// "pub" makes this enum visible to other modules that import this file.
pub enum AluOp: bit[3] {
    Add = 0b000,
    Sub = 0b001,
    And = 0b010,
    Or  = 0b011,
    Xor = 0b100,
    Shl = 0b101,
    Shr = 0b110,
    Eq  = 0b111
}

entity Alu {
    in  clk:       clock,
    in  rst:       reset,
    in  op:        AluOp,
    in  a:         bit[8],
    in  b:         bit[8],
    out result:    bit[8],
    out zero_flag: bit
}

impl Alu {
    // match is an EXPRESSION — it evaluates to a value.
    // Every arm must produce the same type (bit[8] here).
    // The compiler verifies that all 8 variants of AluOp are covered.
    result = match op {
        AluOp::Add => a + b,
        AluOp::Sub => a - b,
        AluOp::And => a & b,
        AluOp::Or  => a | b,
        AluOp::Xor => a ^ b,
        AluOp::Shl => a << b[2:0],
        AluOp::Shr => a >> b[2:0],
        AluOp::Eq  => if a == b { 8'h01 } else { 8'h00 }
    }

    // Combinational flag — derived from the result.
    zero_flag = (result == 0)
}
```

### What Makes This Different from Integer Constants

**The enum is a type.** The port `in op: AluOp` accepts only values of type `AluOp`. You cannot accidentally pass an unrelated `bit[3]` value without an explicit cast. The type system catches wiring mistakes.

**The match is exhaustive.** If you comment out the `AluOp::Shr` arm, the compiler reports:

```
error[E0042]: non-exhaustive match — missing variant `AluOp::Shr`
  --> src/alu.sk:38:5
   |
38 |     result = match op {
   |              ^^^^^ pattern `AluOp::Shr` not covered
```

This is a compile-time error, not a simulation surprise.

**The match is an expression.** Unlike a statement that executes code, `match` evaluates to a value. The entire `match` block on the right side of `result = ...` produces a `bit[8]` value. This means you can nest match expressions, pass them as arguments, or use them anywhere an expression is expected.

**Every arm must agree on the return type.** If one arm returns `bit[8]` and another returns `bit[16]`, the compiler rejects it. This prevents width mismatches that would silently truncate in SystemVerilog.

### What Happens When You Add a Variant

Suppose you later add a new operation:

```
pub enum AluOp: bit[4] {
    Add  = 0b0000,
    Sub  = 0b0001,
    And  = 0b0010,
    Or   = 0b0011,
    Xor  = 0b0100,
    Shl  = 0b0101,
    Shr  = 0b0110,
    Eq   = 0b0111,
    Nand = 0b1000    // new!
}
```

Every `match` on `AluOp` in your entire codebase now fails to compile until you add a `Nand` arm. The compiler finds every place that needs updating. In a large design with dozens of files matching on the same enum, this is invaluable.

### The Catch-All: `_`

Sometimes an enum's underlying bit representation can hold values that do not correspond to any named variant. For example, `AluOp: bit[3]` has 8 possible bit patterns and 8 named variants — a perfect fit. But if you had only 5 variants on a `bit[3]`, there would be 3 unnamed bit patterns. The `_` catch-all handles those:

```
result = match op {
    AluOp::Add => a + b,
    AluOp::Sub => a - b,
    AluOp::And => a & b,
    AluOp::Or  => a | b,
    AluOp::Xor => a ^ b,
    _ => 8'h00  // unnamed bit patterns get a safe default
}
```

Use `_` sparingly. Prefer listing every variant explicitly — it gives you better protection when variants are added later. A `_` arm silently absorbs new variants, which is the exact problem you are trying to avoid.

---

> **Coming from SystemVerilog?**
>
> SystemVerilog has `enum` and `case`, but the guarantees are weaker:
>
> | SystemVerilog | skalp | Why it matters |
> |---|---|---|
> | `case` is a **statement** | `match` is an **expression** (returns a value) | You can assign the result directly: `result = match op { ... }` |
> | `default` masks missing cases silently | Compiler **errors** on missing variants | Adding a new enum variant forces you to handle it everywhere |
> | `unique case` checks at **simulation time** | Exhaustiveness checked at **compile time** | Bugs found before simulation runs, not after hours of waveform debugging |
> | Enum values are integers in the same scope | Variants are namespaced: `AluOp::Add` | No collision between `AluOp::Add` and `FsmState::Add` |
> | Width mismatch silently truncates | All arms must return the same type | No silent data loss from width disagreements |
>
> The biggest shift: in SystemVerilog, `default` in a `case` statement is considered good practice. In skalp, `_` is a last resort. Explicit coverage of every variant is the default expectation. When you add a variant to an SV enum and forget a case arm, `default` handles it silently — your design compiles, simulates, and maybe even synthesises before anyone notices the bug. In skalp, the compiler stops you immediately.

---

## Running Project: Enum-Driven UART

Time to apply enums to the UART project. We make two changes: replace the integer-encoded FSM states with proper enums, and add a command parser that decodes incoming bytes into typed commands.

### Refactoring FSM States

In Chapters 2 and 3, the transmitter and receiver used integer constants for states:

```
signal IDLE:  nat[2] = 0
signal START: nat[2] = 1
signal DATA:  nat[2] = 2
signal STOP:  nat[2] = 3

signal state: nat[2]
```

This is fragile. The number `2` appears in the state signal, the constant declarations, and implicitly in every comparison. Let us replace it with an enum.

Create `src/uart_types.sk` and add the state enums (alongside the structs from Chapter 6):

```
// uart_types.sk — shared types for the UART project

// ---- Structs from Chapter 6 (unchanged) ----

pub struct UartConfig {
    baud_div:     nat[16],
    data_bits:    nat[4],
    parity_en:    bit,
    stop_bits:    bit
}

pub struct UartStatus {
    tx_busy:      bit,
    rx_valid:     bit,
    tx_fifo_full: bit,
    rx_fifo_empty: bit,
    frame_error:  bit,
    overrun:      bit
}

// ---- New: FSM state enums ----

pub enum TxState: bit[2] {
    Idle  = 0,
    Start = 1,
    Data  = 2,
    Stop  = 3
}

pub enum RxState: bit[2] {
    Idle  = 0,
    Start = 1,
    Data  = 2,
    Stop  = 3
}

// ---- New: Command enum ----

pub enum UartCommand: bit[8] {
    Reset     = 0x00,
    SetBaud   = 0x01,
    EnableTx  = 0x02,
    EnableRx  = 0x03,
    Status    = 0x04,
    Loopback  = 0x05,
    Unknown   = 0xFF
}
```

`TxState` and `RxState` are separate enums even though they have the same variants. This is intentional — you cannot accidentally assign a `TxState` to an `RxState` signal. The type system keeps them apart.

### Refactored UART Transmitter

Update `src/uart_tx.sk` to use the enum. Here is the full refactored module:

```
// uart_tx.sk — UART transmitter with enum-driven FSM

entity UartTx<const CYCLES_PER_BIT: nat = 434> {
    in  clk:     clock,
    in  rst:     reset,
    in  tx_data: bit[8],
    in  tx_en:   bit,
    out tx:      bit,
    out tx_busy: bit
}

impl UartTx {
    signal state:        TxState
    signal baud_counter: nat[16]
    signal bit_index:    nat[3]
    signal shift_reg:    bit[8]
    signal tx_out:       bit

    on(clk.rise) {
        if rst {
            state        = TxState::Idle
            baud_counter = 0
            bit_index    = 0
            shift_reg    = 0
            tx_out       = 1
        } else {
            match state {
                TxState::Idle => {
                    tx_out       = 1
                    baud_counter = 0
                    bit_index    = 0

                    if tx_en {
                        shift_reg = tx_data
                        state     = TxState::Start
                    }
                }

                TxState::Start => {
                    // Drive start bit (low)
                    tx_out = 0

                    if baud_counter == CYCLES_PER_BIT - 1 {
                        baud_counter = 0
                        state        = TxState::Data
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }

                TxState::Data => {
                    // Drive current data bit (LSB first)
                    tx_out = shift_reg[0]

                    if baud_counter == CYCLES_PER_BIT - 1 {
                        baud_counter = 0
                        shift_reg    = shift_reg >> 1

                        if bit_index == 7 {
                            state = TxState::Stop
                        } else {
                            bit_index = bit_index + 1
                        }
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }

                TxState::Stop => {
                    // Drive stop bit (high)
                    tx_out = 1

                    if baud_counter == CYCLES_PER_BIT - 1 {
                        baud_counter = 0
                        state        = TxState::Idle
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }
            }
        }
    }

    // Combinational outputs
    tx      = tx_out
    tx_busy = match state {
        TxState::Idle => 0,
        TxState::Start => 1,
        TxState::Data  => 1,
        TxState::Stop  => 1
    }
}
```

Notice two uses of `match`:

1. **Inside the `on(clk.rise)` block** — the state machine transitions. Here `match` is used as a statement-like construct where each arm contains a block of sequential assignments. The compiler still checks exhaustiveness.

2. **In the combinational assignment to `tx_busy`** — a pure expression match. The transmitter is busy in every state except Idle. Every variant is listed explicitly. If someone later added a `TxState::Parity` variant for parity bit support, the compiler would flag both match expressions as incomplete.

Compare this to the Chapter 2 version:

```
// Chapter 2 — integer constants
if state == IDLE {
    ...
} else if state == START {
    ...
} else if state == DATA {
    ...
} else if state == STOP {
    ...
}
```

The old code compiles fine even if you delete one of the branches. The new code does not. That is the point.

### Command Parser

The second use of enums is more interesting. Real UART peripherals often accept commands: reset the controller, change the baud rate, query status. Rather than scattering magic byte comparisons across the design, define a `UartCommand` enum and parse incoming bytes into it.

Add `src/uart_cmd.sk`:

```
// uart_cmd.sk — command decoder for the UART peripheral

entity UartCommandParser {
    in  clk:         clock,
    in  rst:         reset,
    in  rx_data:     bit[8],
    in  rx_valid:    bit,
    out cmd:         UartCommand,
    out cmd_valid:   bit,
    out cmd_data:    bit[8]
}

impl UartCommandParser {
    // State: are we waiting for a command byte, or a data byte?
    signal waiting_for_data: bit
    signal current_cmd:      UartCommand
    signal cmd_out:          UartCommand
    signal valid_out:        bit
    signal data_out:         bit[8]

    // Parse the received byte into a command enum.
    // This is a combinational decode — it happens every cycle,
    // but we only latch the result when rx_valid is high.
    signal parsed_cmd: UartCommand

    parsed_cmd = match rx_data {
        0x00 => UartCommand::Reset,
        0x01 => UartCommand::SetBaud,
        0x02 => UartCommand::EnableTx,
        0x03 => UartCommand::EnableRx,
        0x04 => UartCommand::Status,
        0x05 => UartCommand::Loopback,
        _    => UartCommand::Unknown
    }

    on(clk.rise) {
        // Default: valid is a one-cycle pulse
        valid_out = 0

        if rst {
            waiting_for_data = 0
            current_cmd      = UartCommand::Unknown
            cmd_out          = UartCommand::Unknown
            valid_out        = 0
            data_out         = 0
        } else if rx_valid {
            if !waiting_for_data {
                // First byte: the command itself
                current_cmd = parsed_cmd

                // Some commands need a follow-up data byte
                signal needs_data: bit
                needs_data = match parsed_cmd {
                    UartCommand::SetBaud  => 1,
                    UartCommand::Loopback => 1,
                    _                     => 0
                }

                if needs_data {
                    waiting_for_data = 1
                } else {
                    // Command is complete — emit it now
                    cmd_out   = parsed_cmd
                    valid_out = 1
                    data_out  = 0
                }
            } else {
                // Second byte: the data payload for the command
                cmd_out          = current_cmd
                data_out         = rx_data
                valid_out        = 1
                waiting_for_data = 0
            }
        }
    }

    // Drive output ports
    cmd       = cmd_out
    cmd_valid = valid_out
    cmd_data  = data_out
}
```

Several things to notice:

**The `parsed_cmd` assignment uses `_` correctly.** There are 256 possible `bit[8]` values but only 6 named commands. The `_` catch-all maps everything else to `UartCommand::Unknown`. This is one of the rare cases where `_` is the right choice — you genuinely cannot enumerate all 256 values.

**The `needs_data` match is also exhaustive.** Even though we use `_` for the "no data needed" case, the compiler still knows about every variant. If someone adds `UartCommand::SetParity` later, it falls into `_` and defaults to "no data needed" — which might be wrong. A more defensive version would list every variant explicitly:

```
needs_data = match parsed_cmd {
    UartCommand::SetBaud   => 1,
    UartCommand::Loopback  => 1,
    UartCommand::Reset     => 0,
    UartCommand::EnableTx  => 0,
    UartCommand::EnableRx  => 0,
    UartCommand::Status    => 0,
    UartCommand::Unknown   => 0
}
```

This version forces a compile error when `SetParity` is added, because the match is no longer exhaustive. Whether you use `_` or explicit coverage depends on whether you want new variants to be safe-by-default or loud-by-default. For hardware, loud-by-default is usually better.

**Match inside sequential blocks.** Both match expressions inside the `on(clk.rise)` block work exactly like match in combinational context — they are exhaustive, they return values, and the compiler enforces type consistency. The only difference is that assignments within the arms create registered logic.

### Integrating the Command Parser into UartTop

Update `src/uart_top.sk` to include the command parser alongside the existing TX, RX, and FIFOs:

```
// In UartTop's impl block, add the command parser instance:

let cmd_parser = UartCommandParser {
    clk:       clk,
    rst:       rst,
    rx_data:   rx_fifo_out,
    rx_valid:  rx_fifo_rd_valid,
    cmd:       parsed_command,
    cmd_valid: command_valid,
    cmd_data:  command_data
}

// React to parsed commands
signal parsed_command: UartCommand
signal command_valid:  bit
signal command_data:   bit[8]

on(clk.rise) {
    if command_valid {
        match parsed_command {
            UartCommand::Reset => {
                // Assert internal reset
                soft_reset = 1
            }
            UartCommand::SetBaud => {
                // Update baud rate from command_data
                config.baud_div = command_data[7:0] ++ 8'h00
            }
            UartCommand::EnableTx => {
                tx_enabled = 1
            }
            UartCommand::EnableRx => {
                rx_enabled = 1
            }
            UartCommand::Status => {
                // Queue status byte for transmission
                status_requested = 1
            }
            UartCommand::Loopback => {
                loopback_mode = command_data[0]
            }
            UartCommand::Unknown => {
                // Ignore unknown commands
            }
        }
    }
}
```

Every variant of `UartCommand` has an explicit handler. No `default`, no silent fallthrough. If someone adds a `UartCommand::SetParity` variant next month, this `match` block fails to compile until it is updated. That is the safety guarantee.

---

## Build and Test

Your project structure should now include:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk
    uart_tx.sk         <- updated with TxState enum
    uart_rx.sk         <- updated with RxState enum
    uart_types.sk      <- updated with enums
    fifo.sk
    uart_top.sk        <- updated with command parser
    uart_cmd.sk        <- new
    alu.sk             <- standalone example
```

Compile the standalone ALU example:

```bash
skalp build src/alu.sk
```

Compile the full UART project:

```bash
skalp build
```

The compiler now checks every `match` expression for exhaustiveness. Try commenting out one arm in the ALU match to see the error:

```bash
# Comment out AluOp::Xor arm in alu.sk, then:
skalp build src/alu.sk

# Expected output:
#    error[E0042]: non-exhaustive match — missing variant `AluOp::Xor`
#      --> src/alu.sk:38:5
```

Run the simulation to verify the refactored UART still works:

```bash
skalp sim --entity UartTop --cycles 50000 --vcd build/uart_enum.vcd
```

Open the VCD in GTKWave. The `state` signals now show enum variant names instead of raw integers, making waveform debugging significantly easier.

To test the command parser specifically:

```bash
skalp test src/uart_cmd.sk --trace
```

Verify that sending byte `0x00` produces `UartCommand::Reset`, byte `0x01` produces `UartCommand::SetBaud` (and waits for a data byte), and byte `0x42` produces `UartCommand::Unknown`.

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Enum declaration | `pub enum Name: bit[N] { ... }` | `pub enum TxState: bit[2] { Idle = 0, Start = 1 }` |
| Enum variant value | `Variant = value` | `Add = 0b000` |
| Variant access | `EnumName::Variant` | `AluOp::Add` |
| Match expression | `match expr { Pattern => value, ... }` | `result = match op { AluOp::Add => a + b, ... }` |
| Match in sequential block | `match` inside `on(clk.rise)` | `match state { TxState::Idle => { ... }, ... }` |
| Catch-all arm | `_ => value` | `_ => UartCommand::Unknown` |
| Enum as port type | `in name: EnumType` | `in op: AluOp` |
| Enum as signal type | `signal name: EnumType` | `signal state: TxState` |
| Exhaustiveness error | Compiler rejects missing variants | Comment out one arm to trigger |

---

## Next: Clock Domain Crossing

Your UART now has typed state machines that the compiler can verify, and a command parser that decodes incoming bytes into a safe enum. But there is an assumption baked into the entire design: everything runs on a single clock.

Real UART peripherals sit between two clock domains — the system bus clock (often 100+ MHz) and the UART baud clock (derived from a different oscillator or PLL). Moving data between these domains requires careful synchronization to avoid metastability.

In **[Chapter 8: Clock Domain Crossing](../08-clock-domain-crossing/)**, you will learn how skalp uses **clock lifetimes** to track which clock domain every signal belongs to. The compiler will refuse to connect signals from different domains without an explicit synchronizer. You will build a dual-clock async FIFO and make the UART operate safely across clock boundaries — with the compiler proving correctness rather than relying on linting tools or code review.

Continue to [Chapter 8: Clock Domain Crossing](../08-clock-domain-crossing/).
