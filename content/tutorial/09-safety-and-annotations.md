---
title: "Chapter 9: Safety and Annotations"
date: 2025-07-15
summary: "Add safety mechanisms and debug infrastructure to your designs with TMR voting, detection signals, retention hints, trace grouping, and simulation breakpoints -- all zero-cost annotations that travel with your source code."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 9
ShowToc: true
---

## What This Chapter Teaches

Every previous chapter focused on making the UART work correctly under normal conditions. This chapter shifts perspective: what happens when the hardware itself fails? A cosmic ray flips a register. A voltage glitch corrupts a state machine. A timing violation writes garbage into a FIFO. In safety-critical systems — automotive, aerospace, medical — you must not only detect these faults but also prove that your design handles them.

Traditional hardware design languages have no opinion about safety. You write the safety logic by hand, document it in a spreadsheet, and manually cross-reference the two whenever the design changes. The spreadsheet drifts. The logic evolves. The mapping between them breaks silently.

skalp takes a different approach. Safety properties are annotations in the source code, attached directly to the entities and signals they describe. The compiler reads these annotations, verifies consistency, and can generate FMEDA (Failure Modes, Effects, and Diagnostic Analysis) data automatically. When you change the design, the safety annotations change with it. There is no external spreadsheet to maintain.

This chapter covers two categories of annotations:

**Safety annotations** that feed into fault analysis:

- `#[safety_mechanism]` — marks an entity as a safety mechanism with type metadata (diagnostic coverage is calculated via fault injection)
- `#[detection_signal]` — marks an output that detects faults, telling the fault injection system what to observe
- `#[retention]` — marks state that must persist across clock cycles, flagging it for retention analysis

**Debug annotations** that improve simulation and waveform analysis:

- `#[trace]` — groups signals for waveform visualization with display names and radix control
- `#[breakpoint]` — halts simulation when a condition occurs, with a named error message

All five annotations have zero synthesis cost. The compiler strips them entirely when generating SystemVerilog for synthesis. They exist only in the skalp source and in the compiler's analysis passes.

By the end of this chapter you will understand:

- How Triple Modular Redundancy (TMR) works and why it is the standard pattern for single-fault masking
- How `#[safety_mechanism]` declares what kind of protection an entity provides
- How `#[detection_signal]` connects fault injection to fault observation
- How `#[retention]` flags state that the compiler should verify for persistence
- How `#[trace]` organizes waveform signals into groups that travel with the source code
- How `#[breakpoint]` creates named simulation stop conditions with error messages
- How all of these compose in a real design — the running UART project

The running project adds TMR protection to the UART FSM, parity generation and checking, frame error detection, overrun detection, and a full set of debug traces with breakpoints for critical error conditions.

---

## Standalone Example: TMR Counter

Triple Modular Redundancy is the workhorse of safety-critical hardware. The idea is simple: run three copies of the same logic, compare their outputs with a majority voter, and flag any disagreement. A single fault in any one copy is masked by the other two. The voter always produces the correct output as long as at most one copy is corrupted.

Create a file called `src/tmr_counter.sk`:

```
// A TMR-protected counter with majority voting and error detection.
//
// Three independent counter instances run in parallel. A majority
// voter selects the correct output. Any disagreement between the
// three copies raises tmr_error for one cycle.
//
// The #[safety_mechanism] annotation tells the compiler that this
// entity exists to protect against faults. The type field feeds
// into automated FMEDA generation. Diagnostic coverage is
// calculated automatically via fault injection — not specified here.

#[safety_mechanism(type = tmr)]
entity TmrCounter<const WIDTH: nat = 8> {
    in clk: clock,
    in rst: reset,
    in enable: bit,
    out count: nat[WIDTH],

    #[detection_signal]
    out tmr_error: bit
}

impl TmrCounter {
    // Three independent counter registers.
    // In synthesis, these become three separate flip-flop chains.
    // The compiler does not optimize them into one — the
    // #[safety_mechanism] annotation prevents merging.
    signal count_a: nat[WIDTH]
    signal count_b: nat[WIDTH]
    signal count_c: nat[WIDTH]

    // All three counters run identical logic.
    // In a real TMR implementation, you might also place these
    // in separate clock regions or voltage domains for spatial
    // diversity, but the logic-level redundancy starts here.
    on(clk.rise) {
        if rst {
            count_a = 0
            count_b = 0
            count_c = 0
        } else if enable {
            count_a = count_a + 1
            count_b = count_b + 1
            count_c = count_c + 1
        }
    }

    // Majority voter: selects the value agreed upon by at least
    // two of the three copies.
    //
    // The match expression covers all four cases of pairwise
    // agreement. In the (false, false) case, a and c must agree
    // (since a != b and b != c, the only 2-of-3 majority left
    // is a and c).
    count = match (count_a == count_b, count_b == count_c) {
        (true, true)   => count_a,  // all three agree
        (true, false)  => count_a,  // a and b agree, c diverged
        (false, true)  => count_b,  // b and c agree, a diverged
        (false, false) => count_a   // a and c agree, b diverged
    }

    // Error detection: any disagreement among the three copies
    // means a fault has occurred. This signal does not affect the
    // output — the voter already masked the fault — but it tells
    // the system that a fault was detected and should be logged
    // or acted upon.
    //
    // The #[detection_signal] annotation on the port declaration
    // tells the fault injection framework which output to observe
    // when injecting faults into this entity. Without it, the
    // framework would not know which signal indicates "fault detected."
    tmr_error = (count_a != count_b) | (count_b != count_c)
}
```

### How the Annotations Work

**`#[safety_mechanism(type = tmr)]`** is attached to the entity declaration. It tells the compiler two things:

1. This entity is a safety mechanism — it exists to protect against hardware faults.
2. The protection type is TMR — triple modular redundancy with majority voting.

You do not specify diagnostic coverage in the annotation. Instead, the compiler calculates it automatically using fault injection (FI). When you run `skalp fault-inject`, the tool injects faults into the protected logic and observes whether the `#[detection_signal]` outputs detect them. The measured detection rate is the diagnostic coverage. When you run `skalp fmeda`, the tool walks the design hierarchy, finds every entity with `#[safety_mechanism]`, incorporates the FI-derived coverage, and builds a fault classification table. Entities without safety annotations are classified as unprotected. The coverage numbers feed into the overall safety metrics required by standards like ISO 26262 (automotive) and IEC 61508 (industrial).

**`#[detection_signal]`** is attached to the `tmr_error` output port. It tells the fault injection framework: "when you inject a fault into this entity, check this output to determine whether the fault was detected." Without this annotation, the framework would inject faults but have no way to measure detection coverage automatically. You could still observe any signal manually, but automation requires knowing which signals are detection outputs.

### Adding Debug Annotations

Now let us add debug infrastructure to the same counter. These annotations do not affect safety analysis — they are purely for simulation and waveform viewing.

```
impl TmrCounter {
    // ... (counter logic from above)

    // Trace annotations group signals for waveform display.
    // When you open the generated VCD in a waveform viewer,
    // signals with the same group appear together.

    #[trace(group = "tmr_internals", display_name = "Copy A")]
    signal count_a: nat[WIDTH]

    #[trace(group = "tmr_internals", display_name = "Copy B")]
    signal count_b: nat[WIDTH]

    #[trace(group = "tmr_internals", display_name = "Copy C")]
    signal count_c: nat[WIDTH]

    #[trace(group = "tmr_voted", display_name = "Voted Output", radix = hex)]
    signal voted_count: nat[WIDTH]

    // Breakpoint: halt simulation if a TMR error is detected.
    // The is_error flag tells the simulator to treat this as a
    // failure, not just a stop condition.
    #[breakpoint(is_error = true, name = "TMR_FAULT", message = "TMR voter detected disagreement among counter copies")]
    signal tmr_fault_trigger: bit
    tmr_fault_trigger = tmr_error
}
```

**`#[trace(group = "tmr_internals", display_name = "Copy A")]`** controls how this signal appears in the waveform viewer. The `group` field organizes related signals into a named folder. The `display_name` field overrides the signal's HDL name with a human-readable label. The optional `radix` field controls the display format: `hex`, `bin`, `dec`, or `unsigned`. When you generate a VCD file and open it, the viewer automatically groups these signals together with their display names.

The key difference from traditional waveform setup: in SystemVerilog, waveform grouping lives in the simulator's GUI configuration file. Every engineer sets it up manually. When you share a project, each person recreates the signal groups from scratch. In skalp, the grouping travels with the source code. Clone the repo, run the simulation, open the VCD — the groups are already there.

**`#[breakpoint(is_error = true, name = "TMR_FAULT", message = "...")]`** creates a named simulation stop condition. When `tmr_fault_trigger` goes high during simulation, the simulator halts and prints:

```
BREAKPOINT TMR_FAULT: TMR voter detected disagreement among counter copies
  at cycle 1247, tmr_counter.sk:68
```

The `is_error = true` flag tells the simulator to exit with a non-zero status code, which integrates with CI pipelines. If `is_error` were false, the simulator would halt but report success — useful for "stop here and inspect" breakpoints during manual debugging.

### The Retention Annotation

One more annotation that does not appear in the TMR counter but matters for other designs:

```
#[retention]
signal calibration_value: bit[16]
```

`#[retention]` marks state that must persist across clock cycles — state that the design depends on being stable. The compiler flags any `#[retention]` signal that is conditionally assigned without a default, or that might be clobbered by a reset path that should not touch it. This is a lint-level check, not a synthesis directive. It catches the class of bugs where a register is accidentally cleared by a reset condition that should not affect it.

Retention is particularly important for calibration registers, configuration state loaded at startup, and accumulated statistics. These signals should survive soft resets but might be cleared by a hard reset. The `#[retention]` annotation lets you express that intent, and the compiler verifies it.

> **Coming from SystemVerilog?**
>
> Safety and debug annotations are the area with the largest gap between skalp and traditional HDLs:
>
> | SystemVerilog | skalp | Notes |
> |---------------|-------|-------|
> | No language support | `#[safety_mechanism(type = tmr)]` | Safety analysis lives in spreadsheets in SV |
> | No language support | `#[detection_signal]` | Fault injection mapping is manual in SV |
> | No language support | `#[retention]` | Reset analysis is manual review in SV |
> | Simulator GUI config | `#[trace(group = "...")]` | Waveform grouping travels with source code |
> | `$display`, `$error` | `#[breakpoint(is_error = true)]` | Named, structured, CI-integrated |
> | SVA `assert property` | `#[breakpoint]` (partial overlap) | SVA covers temporal properties; breakpoints are simpler |
>
> The biggest shift is conceptual. In SystemVerilog, safety analysis is a separate discipline performed by safety engineers using spreadsheets and documents. The connection between the RTL and the safety analysis is maintained by humans. When the RTL changes, someone must manually update the FMEDA spreadsheet. This process is slow, error-prone, and frequently out of date.
>
> In skalp, safety metadata is part of the source code. When you add TMR to an entity, you annotate it at the same time. When you refactor, the annotations move with the code. When you run `skalp fmeda`, the tool reads the annotations directly — no spreadsheet synchronization required. The safety analysis is always consistent with the design because they are the same artifact.
>
> For debug, the shift is similar. SystemVerilog waveform setup is per-engineer, per-tool, and per-session. skalp's `#[trace]` annotations are checked into version control. Every engineer who opens the project sees the same signal groups. This eliminates the "how do I set up my waveform viewer" conversation that happens on every project.

---

## Running Project: Safety-Hardened UART

The UART from previous chapters works correctly when all hardware behaves perfectly. Now we add protection against hardware faults and debug infrastructure for development. This touches three areas: TMR on the FSM state registers, parity and framing error detection on the serial data, and trace/breakpoint annotations throughout.

### Part 1: TMR on the TX State Machine

The most critical register in the UART transmitter is the FSM state. If a fault flips the state register, the transmitter can enter an illegal state and corrupt the serial output. TMR protects against this by maintaining three copies of the state and voting on every cycle.

Update `src/uart_tx.sk` to add TMR protection to the state register:

```
// UART Transmitter with TMR-protected FSM state.
//
// The state register is triplicated. A majority voter determines
// the active state on every cycle. Any disagreement raises
// fsm_error for external monitoring.

#[safety_mechanism(type = tmr)]
entity UartTx<
    const CLK_FREQ_HZ: nat = 50_000_000,
    const BAUD_RATE: nat = 115200,
    const DATA_BITS: nat = 8
> {
    in clk: clock,
    in rst: reset,
    in tx_start: bit,
    in tx_data: bit[DATA_BITS],
    out tx_serial: bit,
    out tx_busy: bit,
    out tx_done: bit,

    #[detection_signal]
    out fsm_error: bit,

    out tx_parity: bit
}

impl UartTx {
    const CYCLES_PER_BIT: nat = CLK_FREQ_HZ / BAUD_RATE
    const COUNTER_WIDTH: nat = clog2(CYCLES_PER_BIT)
    const BIT_INDEX_WIDTH: nat = clog2(DATA_BITS + 2)

    // TMR: three copies of the state register.
    signal state_a: TxState
    signal state_b: TxState
    signal state_c: TxState

    // The voted state — used by all downstream logic.
    signal state: TxState

    // Majority voter for FSM state.
    state = match (state_a == state_b, state_b == state_c) {
        (true, true)   => state_a,
        (true, false)  => state_a,
        (false, true)  => state_b,
        (false, false) => state_a
    }

    // Error detection: any state copy disagrees.
    fsm_error = (state_a != state_b) | (state_b != state_c)

    // Internal signals (not triplicated — TMR protects only the
    // state register, which is the most critical single point of
    // failure in the FSM).
    signal baud_counter: nat[COUNTER_WIDTH]
    signal bit_index: nat[BIT_INDEX_WIDTH]
    signal shift_reg: bit[DATA_BITS]
    signal baud_tick: bit

    baud_tick = (baud_counter == CYCLES_PER_BIT - 1)

    // Parity generation: XOR all data bits.
    // This is a combinational reduction — the compiler generates
    // an XOR tree, not a chain.
    tx_parity = tx_data[0] ^ tx_data[1] ^ tx_data[2] ^ tx_data[3] ^
                tx_data[4] ^ tx_data[5] ^ tx_data[6] ^ tx_data[7]

    // Sequential logic: all three state copies are updated
    // with the same next-state logic.
    on(clk.rise) {
        if rst {
            state_a = TxState::Idle
            state_b = TxState::Idle
            state_c = TxState::Idle
            baud_counter = 0
            bit_index = 0
            shift_reg = 0
        } else {
            // Baud counter — shared across all states.
            if baud_tick {
                baud_counter = 0
            } else if state != TxState::Idle {
                baud_counter = baud_counter + 1
            }

            // Next-state logic — computed once, written to all three copies.
            match state {
                TxState::Idle => {
                    tx_serial = 1
                    if tx_start {
                        shift_reg = tx_data
                        state_a = TxState::Start
                        state_b = TxState::Start
                        state_c = TxState::Start
                        baud_counter = 0
                    }
                },
                TxState::Start => {
                    tx_serial = 0
                    if baud_tick {
                        bit_index = 0
                        state_a = TxState::Data
                        state_b = TxState::Data
                        state_c = TxState::Data
                    }
                },
                TxState::Data => {
                    tx_serial = shift_reg[0]
                    if baud_tick {
                        shift_reg = shift_reg >> 1
                        if bit_index == DATA_BITS - 1 {
                            state_a = TxState::Stop
                            state_b = TxState::Stop
                            state_c = TxState::Stop
                        } else {
                            bit_index = bit_index + 1
                        }
                    }
                },
                TxState::Stop => {
                    tx_serial = 1
                    if baud_tick {
                        state_a = TxState::Idle
                        state_b = TxState::Idle
                        state_c = TxState::Idle
                    }
                }
            }
        }
    }

    // Output signals.
    tx_busy = (state != TxState::Idle)
    tx_done = (state == TxState::Stop) & baud_tick
}
```

### Part 2: RX Error Detection

The receiver needs three types of error detection: parity errors (data corruption), framing errors (stop bit not high), and overrun errors (new data arrives when the FIFO is full).

Update `src/uart_rx.sk` to add error detection:

```
// UART Receiver with parity checking, frame error detection,
// and overrun monitoring.

entity UartRx<
    const CLK_FREQ_HZ: nat = 50_000_000,
    const BAUD_RATE: nat = 115200,
    const DATA_BITS: nat = 8
> {
    in clk: clock,
    in rst: reset,
    in rx_serial: bit,
    in expected_parity: bit,
    out rx_data: bit[DATA_BITS],
    out rx_valid: bit,

    #[detection_signal]
    out parity_error: bit,

    #[detection_signal]
    out frame_error: bit
}

impl UartRx {
    const CYCLES_PER_BIT: nat = CLK_FREQ_HZ / BAUD_RATE
    const HALF_BIT: nat = CYCLES_PER_BIT / 2
    const COUNTER_WIDTH: nat = clog2(CYCLES_PER_BIT)
    const BIT_INDEX_WIDTH: nat = clog2(DATA_BITS + 2)

    signal state: RxState
    signal baud_counter: nat[COUNTER_WIDTH]
    signal bit_index: nat[BIT_INDEX_WIDTH]
    signal shift_reg: bit[DATA_BITS]
    signal rx_serial_prev: bit

    // Parity check: XOR all received data bits and compare
    // against expected parity. A mismatch means data corruption.
    signal computed_parity: bit
    computed_parity = shift_reg[0] ^ shift_reg[1] ^ shift_reg[2] ^
                      shift_reg[3] ^ shift_reg[4] ^ shift_reg[5] ^
                      shift_reg[6] ^ shift_reg[7]

    on(clk.rise) {
        if rst {
            state = RxState::Idle
            baud_counter = 0
            bit_index = 0
            shift_reg = 0
            rx_valid = 0
            parity_error = 0
            frame_error = 0
            rx_serial_prev = 1
        } else {
            rx_serial_prev = rx_serial
            rx_valid = 0
            parity_error = 0
            frame_error = 0

            match state {
                RxState::Idle => {
                    if rx_serial_prev & !rx_serial {
                        state = RxState::Start
                        baud_counter = 0
                    }
                },
                RxState::Start => {
                    if baud_counter == HALF_BIT - 1 {
                        if !rx_serial {
                            baud_counter = 0
                            bit_index = 0
                            state = RxState::Data
                        } else {
                            state = RxState::Idle
                        }
                    } else {
                        baud_counter = baud_counter + 1
                    }
                },
                RxState::Data => {
                    if baud_counter == CYCLES_PER_BIT - 1 {
                        baud_counter = 0
                        shift_reg = (rx_serial << (DATA_BITS - 1))
                                  | (shift_reg >> 1)
                        if bit_index == DATA_BITS - 1 {
                            state = RxState::Stop
                        } else {
                            bit_index = bit_index + 1
                        }
                    } else {
                        baud_counter = baud_counter + 1
                    }
                },
                RxState::Stop => {
                    if baud_counter == CYCLES_PER_BIT - 1 {
                        if rx_serial {
                            // Valid stop bit — emit byte.
                            rx_data = shift_reg
                            rx_valid = 1

                            // Check parity.
                            if computed_parity != expected_parity {
                                parity_error = 1
                            }
                        } else {
                            // Stop bit is low — framing error.
                            frame_error = 1
                        }
                        state = RxState::Idle
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }
            }
        }
    }
}
```

### Part 3: Overrun Detection in UART Top

The overrun condition happens at the system level: the RX produces valid data, but the FIFO is already full. The new byte is lost. This is detected in `UartTop` where the FIFO and receiver are wired together.

Add overrun detection and debug annotations to `src/uart_top.sk`:

```
entity UartTop<
    const CLK_FREQ_HZ: nat = 50_000_000,
    const BAUD_RATE: nat = 115200,
    const DATA_BITS: nat = 8,
    const FIFO_DEPTH: nat = 16
> {
    in clk: clock,
    in rst: reset,

    // TX interface
    in tx_data: bit[DATA_BITS],
    in tx_valid: bit,
    out tx_ready: bit,

    // RX interface
    out rx_data: bit[DATA_BITS],
    out rx_valid: bit,
    in rx_read: bit,

    // Serial lines
    out tx_serial: bit,
    in rx_serial: bit,

    // Status
    out tx_fifo_full: bit,
    out rx_fifo_empty: bit,

    // Error outputs
    #[detection_signal]
    out tx_fsm_error: bit,

    #[detection_signal]
    out rx_parity_error: bit,

    #[detection_signal]
    out rx_frame_error: bit,

    #[detection_signal]
    out rx_overrun: bit
}

impl UartTop {
    const CYCLES_PER_BIT: nat = CLK_FREQ_HZ / BAUD_RATE
    const HALF_BIT: nat = CYCLES_PER_BIT / 2
    const COUNTER_WIDTH: nat = clog2(CYCLES_PER_BIT)
    const FIFO_ADDR_WIDTH: nat = clog2(FIFO_DEPTH)

    // ── Sub-entity Instantiation ───────────────────────────────

    let uart_tx = UartTx<CLK_FREQ_HZ, BAUD_RATE, DATA_BITS> {
        clk: clk,
        rst: rst,
        tx_start: tx_fifo_read_valid,
        tx_data: tx_fifo_data,
        tx_serial: tx_serial,
        tx_busy: tx_busy_internal,
        tx_done: tx_done_internal,
        fsm_error: tx_fsm_error,
        tx_parity: tx_parity_internal
    }

    let uart_rx = UartRx<CLK_FREQ_HZ, BAUD_RATE, DATA_BITS> {
        clk: clk,
        rst: rst,
        rx_serial: rx_serial,
        expected_parity: 0,
        rx_data: rx_byte_data,
        rx_valid: rx_byte_valid,
        parity_error: rx_parity_error,
        frame_error: rx_frame_error
    }

    let tx_fifo = FIFO<DATA_BITS, FIFO_DEPTH> {
        clk: clk,
        rst: rst,
        write_en: tx_valid & tx_ready,
        write_data: tx_data,
        read_en: tx_fifo_read,
        read_data: tx_fifo_data,
        full: tx_fifo_full,
        empty: tx_fifo_empty_internal
    }

    let rx_fifo = FIFO<DATA_BITS, FIFO_DEPTH> {
        clk: clk,
        rst: rst,
        write_en: rx_write_en,
        write_data: rx_byte_data,
        read_en: rx_read,
        read_data: rx_data,
        full: rx_fifo_full_internal,
        empty: rx_fifo_empty
    }

    // ── Internal Signals ───────────────────────────────────────

    signal tx_busy_internal: bit
    signal tx_done_internal: bit
    signal tx_parity_internal: bit
    signal tx_fifo_data: bit[DATA_BITS]
    signal tx_fifo_empty_internal: bit
    signal tx_fifo_read: bit
    signal tx_fifo_read_valid: bit

    signal rx_byte_data: bit[DATA_BITS]
    signal rx_byte_valid: bit
    signal rx_fifo_full_internal: bit
    signal rx_write_en: bit

    // ── TX Path Control ────────────────────────────────────────

    tx_ready = !tx_fifo_full
    tx_fifo_read = !tx_fifo_empty_internal & !tx_busy_internal
    tx_fifo_read_valid = tx_fifo_read

    // ── RX Path Control ────────────────────────────────────────

    rx_valid = !rx_fifo_empty

    // Write to RX FIFO only if it is not full.
    // If it IS full and new data arrives, that is an overrun.
    rx_write_en = rx_byte_valid & !rx_fifo_full_internal

    // ── Overrun Detection ──────────────────────────────────────

    // Overrun occurs when the receiver produces valid data but
    // the FIFO has no room. The byte is lost.
    #[breakpoint(is_error = true, name = "FIFO_OVERRUN", message = "RX FIFO overrun — data lost")]
    signal rx_overrun_detect: bit
    rx_overrun_detect = rx_byte_valid & rx_fifo_full_internal
    rx_overrun = rx_overrun_detect

    // ── Retention: Error Counters ──────────────────────────────

    // Sticky error registers: once set, they remain high until
    // explicitly cleared by reset. This ensures transient errors
    // are not missed by slow-polling software.

    #[retention]
    signal parity_error_sticky: bit

    #[retention]
    signal frame_error_sticky: bit

    #[retention]
    signal overrun_error_sticky: bit

    on(clk.rise) {
        if rst {
            parity_error_sticky = 0
            frame_error_sticky = 0
            overrun_error_sticky = 0
        } else {
            if rx_parity_error {
                parity_error_sticky = 1
            }
            if rx_frame_error {
                frame_error_sticky = 1
            }
            if rx_overrun_detect {
                overrun_error_sticky = 1
            }
        }
    }

    // ── Debug Trace Annotations ────────────────────────────────
    //
    // These annotations organize signals into groups for waveform
    // viewing. They have zero synthesis cost — stripped entirely
    // during compilation. But they make simulation debugging
    // dramatically faster because every engineer who opens the
    // VCD sees the same organized signal groups.

    #[trace(group = "uart_tx", display_name = "TX State")]
    signal tx_state_trace: TxState

    #[trace(group = "uart_tx", display_name = "TX Baud Tick")]
    signal tx_baud_tick_trace: bit

    #[trace(group = "uart_tx", display_name = "TX Serial Out")]
    signal tx_serial_trace: bit

    #[trace(group = "uart_tx", display_name = "TX Parity", radix = bin)]
    signal tx_parity_trace: bit

    #[trace(group = "uart_rx", display_name = "RX Data Valid")]
    signal rx_valid_trace: bit

    #[trace(group = "uart_rx", display_name = "RX Data", radix = hex)]
    signal rx_data_trace: bit[DATA_BITS]

    #[trace(group = "uart_rx", display_name = "RX Parity Error")]
    signal rx_parity_err_trace: bit

    #[trace(group = "uart_rx", display_name = "RX Frame Error")]
    signal rx_frame_err_trace: bit

    #[trace(group = "uart_errors", display_name = "TX FSM Error")]
    signal tx_fsm_err_trace: bit

    #[trace(group = "uart_errors", display_name = "RX Overrun")]
    signal rx_overrun_trace: bit

    #[trace(group = "uart_errors", display_name = "Parity Sticky")]
    signal parity_sticky_trace: bit

    #[trace(group = "uart_errors", display_name = "Frame Sticky")]
    signal frame_sticky_trace: bit

    // Connect trace signals to actual signals.
    tx_state_trace = uart_tx.state
    tx_baud_tick_trace = uart_tx.baud_tick
    tx_serial_trace = tx_serial
    tx_parity_trace = tx_parity_internal
    rx_valid_trace = rx_byte_valid
    rx_data_trace = rx_byte_data
    rx_parity_err_trace = rx_parity_error
    rx_frame_err_trace = rx_frame_error
    tx_fsm_err_trace = tx_fsm_error
    rx_overrun_trace = rx_overrun_detect
    parity_sticky_trace = parity_error_sticky
    frame_sticky_trace = frame_error_sticky

    // ── Additional Breakpoints ─────────────────────────────────

    #[breakpoint(is_error = true, name = "FRAME_ERROR", message = "RX framing error — stop bit not high")]
    signal frame_err_break: bit
    frame_err_break = rx_frame_error

    #[breakpoint(is_error = false, name = "TX_COMPLETE", message = "TX transmission complete")]
    signal tx_complete_break: bit
    tx_complete_break = tx_done_internal
}
```

### What We Added

Let us step back and see the full picture of what the annotations provide:

**Safety mechanism chain.** `UartTx` is marked `#[safety_mechanism(type = tmr)]`. Its `fsm_error` output is marked `#[detection_signal]`. When you run `skalp fmeda`, the tool knows: "UartTx is a TMR-protected entity. Faults in the state register are masked by the voter. Detection is reported on `fsm_error`." The diagnostic coverage is calculated automatically by the fault injection system — you never specify it manually. This feeds directly into an ISO 26262 safety case.

**Error detection outputs.** The UART top-level exports four detection signals: `tx_fsm_error`, `rx_parity_error`, `rx_frame_error`, and `rx_overrun`. A system-level safety monitor can observe these and take corrective action — reset the UART, log the fault, or escalate to a higher-level safety controller. Each is marked `#[detection_signal]` so the fault injection framework can automate coverage measurement.

**Sticky error registers.** Parity, frame, and overrun errors are pulsed signals — they go high for one cycle when the error occurs. If the CPU polls the status register at a slower rate, it might miss a transient error. The `#[retention]` sticky registers latch the error and hold it until reset. The `#[retention]` annotation ensures the compiler does not accidentally clear these registers in a code path that should preserve them.

**Organized debug traces.** Three trace groups — `uart_tx`, `uart_rx`, and `uart_errors` — organize the most important signals for debugging. When you open the VCD file, you immediately see the TX state machine, RX data flow, and error conditions without manually hunting through hundreds of signals. This setup is checked into version control and shared by the entire team.

**Simulation breakpoints.** Two breakpoints trigger during simulation. `FIFO_OVERRUN` halts with an error if the RX FIFO overflows — this is always a bug in the testbench or a design misconfiguration. `FRAME_ERROR` halts with an error on framing violations. `TX_COMPLETE` halts without an error when a transmission finishes — useful for stepping through individual bytes during manual debugging.

---

## Build and Test

Your project structure should now look like this:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk         (Chapter 1)
    uart_tx.sk          (Chapter 2, updated with TMR and parity)
    uart_rx.sk          (Chapter 3, updated with error detection)
    fifo.sk             (Chapter 4)
    uart_top.sk         (updated with safety and debug annotations)
    adder.sk            (Chapter 5)
    tmr_counter.sk      (this chapter's standalone example)
```

Build the safety-hardened design:

```bash
skalp build
```

The compiler processes all annotations. Safety annotations are validated — if you mark `#[detection_signal]` on a port that does not exist, or attach `#[safety_mechanism]` to an entity without any detection signals, the compiler warns you. Trace and breakpoint annotations are recorded for simulation but produce no synthesis output.

Run the FMEDA analysis:

```bash
skalp fmeda --output build/fmeda_report.csv
```

This scans the design hierarchy, finds every `#[safety_mechanism]` entity, lists its detection signals, and generates a CSV with fault classifications and coverage percentages. The output can be imported directly into a safety case document or reviewed by a safety engineer.

Run simulation with breakpoints enabled:

```bash
skalp sim --entity UartTop \
    --params "CLK_FREQ_HZ=1000,BAUD_RATE=100,FIFO_DEPTH=4" \
    --cycles 2000 \
    --breakpoints \
    --vcd build/uart_safety.vcd
```

The `--breakpoints` flag enables all `#[breakpoint]` annotations. Without it, breakpoints are ignored and simulation runs to completion. This lets you disable breakpoints for long regression runs and enable them for interactive debugging.

If no faults occur, the simulation runs to completion. If a framing error, parity error, or FIFO overrun occurs, the simulation halts with the breakpoint name and message:

```
BREAKPOINT FIFO_OVERRUN at cycle 847: RX FIFO overrun — data lost
  at uart_top.sk:164
Simulation FAILED (breakpoint with is_error = true)
```

Open the VCD file in a waveform viewer. You should see three organized groups: `uart_tx` with the TX state and baud tick, `uart_rx` with the RX data and validity, and `uart_errors` with all error signals and sticky registers. No manual viewer configuration needed.

To run fault injection and verify TMR coverage:

```bash
skalp fault-inject --entity UartTx \
    --target "state_a" \
    --method bit_flip \
    --cycles 500 \
    --runs 1000
```

This flips a random bit in `state_a` at a random cycle across 1000 simulation runs. For each run, the tool checks whether `fsm_error` (the `#[detection_signal]`) went high. The measured detection rate across all runs is the diagnostic coverage for this safety mechanism. This FI-derived coverage is what `skalp fmeda` uses when generating the FMEDA report — no manual coverage numbers needed.

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Safety mechanism | `#[safety_mechanism(type = T)]` | `#[safety_mechanism(type = tmr)]` |
| Detection signal | `#[detection_signal]` | `#[detection_signal] out error: bit` |
| Retention | `#[retention]` | `#[retention] signal cal_value: bit[16]` |
| Trace group | `#[trace(group = "G")]` | `#[trace(group = "pipeline")]` |
| Trace display name | `#[trace(display_name = "Label")]` | `#[trace(display_name = "Stage 1")]` |
| Trace radix | `#[trace(radix = R)]` | `#[trace(radix = hex)]` — also `bin`, `dec`, `unsigned` |
| Combined trace | `#[trace(group = "G", display_name = "L", radix = R)]` | `#[trace(group = "uart_tx", display_name = "TX Data", radix = hex)]` |
| Breakpoint (error) | `#[breakpoint(is_error = true, name = "N", message = "M")]` | `#[breakpoint(is_error = true, name = "OVERFLOW", message = "Counter overflow")]` |
| Breakpoint (info) | `#[breakpoint(is_error = false, name = "N", message = "M")]` | `#[breakpoint(is_error = false, name = "DONE", message = "Transfer complete")]` |
| TMR voter pattern | `match (a == b, b == c) { ... }` | See standalone example above |
| Parity computation | XOR reduction | `p = d[0] ^ d[1] ^ ... ^ d[7]` |
| FMEDA generation | `skalp fmeda` | `skalp fmeda --output report.csv` |
| Fault injection | `skalp fault-inject` | `skalp fault-inject --entity E --target "sig" --method bit_flip` |

---

## Next: Testing and Verification

The UART is now feature-complete. It transmits and receives serial data with baud rate generation, buffers bytes with parameterized FIFOs, structures its interfaces with structs and enums, crosses clock domains safely, and protects its critical state with TMR and error detection. The safety annotations feed into automated FMEDA generation. The debug annotations make simulation practical.

But none of this matters if it is not tested. How do you know the TMR voter actually masks faults? How do you verify that parity errors are detected correctly? How do you confirm that the FIFO overrun breakpoint fires at the right time?

In Chapter 10, you will build a complete **Rust testbench** for the UART. skalp's test framework uses Rust — the same language the compiler is written in — to drive stimulus, check results, and measure coverage. You will write tests for normal operation, error injection, boundary conditions, and timing corner cases. The testbench will exercise every feature you have built across the entire tutorial.

Continue to [Chapter 10: Testing and Verification](../10-testing/).
