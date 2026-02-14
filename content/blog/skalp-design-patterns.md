---
title: "Design Patterns in Real skalp Code"
date: 2025-07-01
summary: "What does production skalp code actually look like? A tour of design patterns from two real projects — a grid-tie inverter control system and a content-addressed processor — covering state machines, type-safe control loops, hierarchical composition, and more."
tags: ["skalp", "hdl", "hardware", "design-patterns"]
ShowToc: true
---

Most HDL documentation shows toy examples — a counter, a FIFO, an ALU. These are useful for learning syntax but don't show you how to structure a real design. What happens when you have a 9-state power converter controller with cascaded PI loops, fault latching, BMS integration, and dual-FPGA lockstep? Or a 43-operation pipelined processor with content-addressed memory, power-gated function units, and zero-cycle morphing?

This post walks through design patterns from two real skalp projects:

**Sangam** (संगम — "confluence"): A modular grid-tie inverter control system for 5-6kW power conversion, targeting a Lattice ECP5-12 FPGA at 100MHz. It controls a Dual Active Bridge (DAB) for bidirectional DC-DC conversion with battery charging (CC-CV-Float for LiFePO4), MPPT solar tracking, and grid PFC — plus comprehensive protection and dual-FPGA lockstep for safety.

**Karythra**: A 256-bit content-addressed processor where everything is identified by Blake3 hashes instead of memory addresses. The CLE (Content-Addressed Logic Element) is a morphable function unit with 43 hardwired operations across 5 tiers, a 4-stage pipeline at 2.5GHz, power-gated domains, and both synchronous and NCL (async) implementations from the same source.

These are not demo projects. They have workarounds for compiler bugs, design decisions driven by real hardware constraints, and the kind of structural patterns that only emerge in non-trivial designs.

---

## Type Aliases for Domain Clarity

The first thing you notice in production skalp code is aggressive use of type aliases:

```
pub type MilliVolts = nat[16]
pub type MilliAmps = int[16]
pub type Watts = int[16]
pub type DeciCelsius = int[16]
pub type AdcRaw = nat[12]
pub type PhaseShift = int[10]
```

These are all just integers underneath, but the aliases serve two purposes. First, they document intent — when you see `signal v_bat_mv: MilliVolts`, you know exactly what it represents without reading comments. Second, they catch category errors during code review. If someone writes `temperature = voltage + current`, the types won't stop the compiler (they're all integers), but the aliases make the mistake visible to a human.

Karythra does the same thing at a different scale:

```
pub type Hash = bit[256]
pub type DataWord = bit[64]
pub type FuncSel = bit[6]
```

The pattern extends to fixed-point types, where the alias encodes the numerical format:

```
/// Q16.16 signed fixed-point (32-bit total)
/// Range: -32768.0 to +32767.99998
/// Resolution: 0.0000153 (1/65536)
pub type q16_16 = fixed<32, 16, true>

pub type q8_8 = fixed<16, 8, true>
pub type q1_15 = fixed<16, 15, true>
```

This matters in power electronics where mixing Q8.8 coefficients with Q16.16 integrator values without proper scaling produces silent numerical errors that manifest as motor oscillation or battery overcharge. The type system makes the fixed-point format explicit at every signal declaration.

---

## State Machines with Match Expressions

Both projects use enum-based state machines with `match` expressions. Here's the battery controller's main state machine — 9 states managing the full power-up sequence:

```
pub enum ModuleState: bit[4] {
    Init = 0,
    WaitBms = 1,
    Precharge = 2,
    SoftStart = 3,
    Running = 4,
    Standby = 5,
    Derate = 6,
    Fault = 7,
    Shutdown = 8
}
```

The state transition logic lives in an `on(clk.rise)` block with nested `match`:

```
on(clk.rise) {
    if rst {
        state_reg = ModuleState::Init
        state_timer = 0
    } else {
        state_timer = state_timer + 1

        match state_reg {
            ModuleState::Init => {
                master_enable = 0
                contactor_close = 0
                if enable && !any_fault_flag {
                    state_reg = ModuleState::WaitBms
                    state_timer = 0
                }
            },

            ModuleState::WaitBms => {
                if !enable || any_fault_flag {
                    state_reg = ModuleState::Fault
                } else if bms_connected && !bms_fault_flag {
                    state_reg = ModuleState::Precharge
                    state_timer = 0
                } else if state_timer > BMS_TIMEOUT_CYCLES {
                    state_reg = ModuleState::Fault
                }
            },

            // ... Precharge, SoftStart, Running, Standby, Derate, Fault, Shutdown
        }
    }
}
```

Two patterns worth noting:

**Default timer increment.** The `state_timer = state_timer + 1` runs before the `match`, so every state gets a free-running timer. States that need to reset it on entry do so explicitly (`state_timer = 0`). This avoids duplicating the increment in every state arm.

**Per-state output assignments.** Outputs like `master_enable` and `contactor_close` are set in every state that cares about them. This makes the behavior of each state self-contained — you can read one match arm and know exactly what that state does without scanning the rest.

Karythra uses a different state machine pattern — pipeline stages with valid flags instead of explicit FSM states:

```
on(clk_fabric.rise) {
    if rst {
        pipe1_valid = 0
    } else if execute_enable {
        pipe1_valid = 1
        pipe1_hash1 = rs1_hash
        pipe1_func = function_sel
    } else {
        pipe1_valid = 0
    }
}
```

Each pipeline stage captures its inputs on the clock edge and passes a `valid` flag forward. The "state" is implicit in which pipeline registers contain valid data. This is the natural pattern for data-flow architectures where you don't have a central controller — the data moves through the pipeline and each stage acts on what it receives.

---

## Hierarchical Composition

skalp uses `let` bindings to instantiate sub-entities with named port connections:

```
let protection = ProtectionSystem {
    clk: clk,
    rst: rst,
    voltage: v_bat_mv,
    current: i_bat_ma,
    temperature: temp_max,
    hw_ov: hw_ov,
    hw_uv: hw_uv,
    hw_oc: hw_oc,
    hw_ot: hw_ot,
    thresholds: config.protection,
    clear_faults: !enable,
    faults: prot_faults,
    any_soft_fault: prot_soft,
    any_hard_fault: prot_hard,
    power_limit: power_limit,
    shutdown_required: shutdown_req
}
```

The named syntax makes port connections self-documenting. Compare this with SystemVerilog's positional port mapping where a reordering silently breaks everything.

The sangam battery controller's hierarchy shows how real designs compose:

```
DabBatteryController (top)
├── ProtectionSystem
│   ├── ThresholdComparator × 5 (OV, UV, OC, OT, desat)
│   └── FaultLatch × 5 (with debounce and auto-clear)
├── CcCvController
│   ├── PiController (voltage loop)
│   └── PiController (current loop)
├── DabPwmGenerator
│   ├── TriangularCarrier
│   ├── HBridgePwm (primary)
│   │   └── HalfBridgePwm × 2
│   └── HBridgePwm (secondary)
│       └── HalfBridgePwm × 2
└── Lockstep comparison logic
```

About 15 leaf instances in the default configuration. Each is a self-contained entity with its own ports, testable in isolation. The composition pattern is always the same: `let name = Entity { ports }`, then access outputs with `name.output`.

---

## Structs for Configuration and Status Aggregation

When entities have many related signals, structs group them:

```
pub struct FaultFlags {
    ov: bit,
    uv: bit,
    oc: bit,
    ot: bit,
    desat: bit,
    bms_fault: bit,
    bms_timeout: bit,
    lockstep: bit
}

pub struct BatteryConfig {
    tap_48v: bit,
    voltage_loop: PiCoefficients,
    current_loop: PiCoefficients,
    protection: ProtectionThresholds,
    switching_freq_khz: nat[8],
    dead_time_ns: nat[8],
    bms_timeout_ms: nat[16]
}
```

Struct construction at the output boundary aggregates signals from different sources:

```
faults = FaultFlags {
    ov: prot_faults.ov,
    uv: prot_faults.uv,
    oc: prot_faults.oc,
    ot: prot_faults.ot,
    desat: prot_faults.desat,
    bms_fault: bms_fault_flag,
    bms_timeout: bms_timeout,
    lockstep: lockstep_fault
}
```

The first five fields come from the protection subsystem, the last three from BMS and lockstep logic. Without structs, this would be 8 separate output ports. With structs, the consumer gets a single `faults` signal and accesses `faults.ov`, `faults.lockstep`, etc.

---

## Generic Parameters for Test vs. Production

One of the most practical patterns in sangam: the same entity serves simulation and real-time by parameterizing timing constants:

```
entity DabBatteryController<
    SOFT_START_CYCLES: nat[32] = SOFT_START_DURATION,
    PRECHARGE_CYCLES: nat[32] = 1000,
    BMS_TIMEOUT_CYCLES: nat[32] = BMS_TIMEOUT,
    CC_CV_FLOAT_ENTRY_CYCLES: nat[32] = FLOAT_ENTRY_DELAY
> {
    // ... ports and logic
}
```

Production instantiation uses the defaults (1 second BMS timeout at 100MHz = 100,000,000 cycles). The test wrapper overrides them:

```
let inner = DabBatteryController::<1000, 1000, 10000, 100> {
    // ... same ports
}
```

Now BMS timeout is 10,000 cycles instead of 100,000,000. The simulation completes in seconds instead of simulating a full second of real time. The logic is identical — only the timing constants change. Monomorphization generates a specialized version for each set of parameters.

Karythra uses generics for data width parameterization:

```
pub fn exec_l0_l1<const W: nat>(opcode: bit[6], data1: bit[64], data2: bit[64]) -> bit[64]

// Called as:
l0_l1_result = exec_l0_l1::<WORD_SIZE>(function_sel, data1, data2)
```

Where `WORD_SIZE` is 32. The function body uses `W` for bit extractions and arithmetic widths. A future 64-bit version is a single constant change.

---

## Control Loops: PI with Anti-Windup

The PI controller is the workhorse of power electronics. Sangam's implementation shows several skalp idioms at once:

```
pub entity PiController {
    in clk: clock
    in rst: reset(active_high)
    in enable: bit
    in update: bit
    in setpoint: q16_16
    in feedback: q16_16
    in kp: q8_8
    in ki: q8_8
    in out_min: q16_16
    in out_max: q16_16
    out output: q16_16
    out saturated: bit
}

impl PiController {
    #[retention]
    signal int_accum: q16_16

    error = setpoint - feedback
    prop_term = (kp * error) >> 8

    on(clk.rise) {
        if rst || reset_integrator {
            int_accum = 0
        } else if enable && update {
            let should_integrate = if saturated {
                if sum_raw > out_max { error < 0 }
                else { error > 0 }
            } else { 1 }

            if should_integrate {
                let delta = (ki * error) >> 8
                int_accum = int_accum + delta
            }
        }
    }

    sum_raw = prop_term + int_accum
    output = if sum_raw > out_max { out_max }
             else if sum_raw < out_min { out_min }
             else { sum_raw }
    saturated = (sum_raw > out_max) || (sum_raw < out_min)
}
```

**`#[retention]`** marks state that persists across cycles. The integrator accumulator is the only stateful element.

**`update` gating.** The PI only computes on ADC valid strobes, not every clock cycle. This decouples the control loop rate from the clock frequency.

**Anti-windup.** When the output is saturated, the integrator only accumulates if the error would reduce saturation (negative error when output is pegged high, positive when pegged low). This prevents integral windup where the accumulator runs away during saturation and causes overshoot when the constraint releases.

**Forward references.** The `saturated` signal is used inside the `on(clk.rise)` block but defined combinationally after it. skalp allows this — combinational signals can be referenced before their definition because they have no temporal ordering.

**Fixed-point scaling.** `(kp * error) >> 8` multiplies a Q8.8 coefficient by a Q16.16 value and right-shifts by 8 to maintain the Q16.16 format. The shift is the fixed-point equivalent of dividing by 2^8 to normalize the product. This pattern appears throughout: multiply, then shift to maintain scale.

The cascaded controller feeds the outer loop's output as the inner loop's setpoint:

```
let outer_loop = PiController { setpoint: v_target, feedback: v_bat, ... }

current_reference = if outer_loop.output < current_limit {
    outer_loop.output
} else {
    current_limit
}

let inner_loop = PiController { setpoint: current_reference, feedback: i_bat, ... }
```

Outer voltage loop → clamp to BMS current limit → inner current loop. Two entity instantiations, one line of limiting logic between them.

---

## Protection: Hysteresis and Fault Latching

Hardware protection needs hysteresis to prevent chatter around thresholds:

```
pub entity ThresholdComparator {
    in value: nat[16]
    in threshold: nat[16]
    in hysteresis: nat[16]
    in compare_high: bit
    out triggered: bit
}

impl ThresholdComparator {
    signal state: bit

    on(clk.rise) {
        if rst { state = 0 }
        else {
            if compare_high {
                if !state { state = value > threshold }
                else { state = value > (threshold - hysteresis) }
            } else {
                if !state { state = value < threshold }
                else { state = value < (threshold + hysteresis) }
            }
        }
    }
    triggered = state
}
```

The comparator triggers at `threshold` but doesn't release until the value crosses `threshold ± hysteresis`. This prevents a noisy signal near the threshold from rapidly toggling the output.

Faults are latched with debounce and auto-clear:

```
pub entity FaultLatch {
    in fault_in: bit
    in clear: bit
    in auto_clear_enable: bit
    in auto_clear_delay: nat[16]
    out fault_out: bit
    out fault_count: nat[8]
}
```

Once a fault fires, it latches and stays latched until explicitly cleared or until auto-clear counts down (only if the underlying condition has resolved). The `fault_count` accumulates how many times the fault has triggered — useful for degraded operation decisions (one OV event might be a transient; ten means something is wrong).

The protection system composes five `ThresholdComparator` + `FaultLatch` pairs (OV, UV, OC, OT, desaturation) into a single `ProtectionSystem` entity with aggregate outputs. The top-level controller only sees `any_hard_fault` and `any_soft_fault` — the internal structure is encapsulated.

---

## Parallel Pre-Computation with Muxing

Karythra computes all function unit results in parallel and selects the right one:

```
l0_l1_result = exec_l0_l1::<WORD_SIZE>(function_sel, data1, data2)
l2_result = exec_l2(function_sel, data1, data2)
l3_result = exec_l3(function_sel, data1, data2)
l4_l5_result = exec_l4_l5(function_sel, data1, data2)

fu_result = if function_sel < 18 {
    l0_l1_result
} else if function_sel < 28 {
    l2_result
} else if function_sel < 38 {
    l3_result
} else {
    l4_l5_result
}
```

This looks wasteful — why compute all four when only one is needed? In hardware, this is the natural pattern. All four function units exist as physical circuits. They all receive the inputs and all produce outputs every cycle. The `if-else` chain becomes a multiplexer that selects one result. Synthesis tools recognize the mutual exclusivity and optimize away any logic that has no path to the selected output.

The alternative — gating inputs to unused function units — saves switching power but adds complexity. With power domain annotations, skalp handles this at a higher level:

```
#[power_domain(id = 1, wake_cycles = 4)]
pub entity FunctionUnitL2<'clk> {
    // ... floating-point operations
}
```

L0-L1 is always on. L2 through L5 can be power-gated when the workload doesn't need them, with a 4-cycle wake-up penalty.

---

## Zero-Cycle Morphing with Shadow Registers

Karythra's CLE can change its function between quantum boundaries (execution windows) with zero latency using shadow registers:

```
#[retention]
signal config_active: bit[9]    // current function
#[retention]
signal config_shadow: bit[9]    // preloaded for next quantum

// Shadow register loads any time
on(clk_fabric.rise) {
    config_shadow = config_next
}

// Swap on quantum boundary — zero cycle
on(clk_fabric.rise) {
    if morph_trigger {
        config_active = config_shadow
    }
}

function_sel = config_active[8:3]
route_sel = config_active[2:0]
```

The configuration for the next quantum is written into `config_shadow` at any point during the current quantum. When `morph_trigger` fires, the active configuration swaps in one cycle — no pipeline flush, no reconfiguration delay. The function unit selection and routing change instantly.

This is a general pattern for any hardware that needs to reconfigure without downtime: maintain a shadow copy, write it asynchronously, swap atomically on a trigger.

---

## Same Source, Sync and Async

Karythra has both a synchronous and an NCL (async) implementation of the CLE, and the computational logic is identical:

```
// Synchronous version (main.sk)
entity KarythraCLE {
    in clk_fabric: clock<'fabric>
    in rst: bit
    // ... ports
}

// Async version (main_async.sk)
async entity KarythraCLEAsync {
    in data1: bit[64]
    in data2: bit[64]
    in function_sel: bit[6]
    out result: bit[64]
    out result_valid: bit
}
```

Both call the same function units:

```
l0_l1_result = exec_l0_l1::<WORD_SIZE>(function_sel, data1, data2)
l2_result = exec_l2(function_sel, data1, data2)
// ...
```

The synchronous version wraps this in pipeline stages with clock edges. The async version is purely combinational — the compiler handles dual-rail expansion and completion detection. The shared function units (`exec_l0_l1`, `exec_l2`, etc.) don't know or care whether they're being used in a clocked or clockless context. They're just combinational logic.

This is a powerful pattern: write the computational core once as pure functions, then wrap it in either synchronous pipeline registers or NCL encode/decode boundaries depending on the target architecture.

---

## Safety Annotations

Karythra's register file demonstrates safety-critical annotations:

```
#[safety_mechanism(type = ecc, coverage = 99.9)]
pub entity EccRegisterFile<'clk, const DEPTH: nat, const WIDTH: nat> {
    in clk: clock<'clk>
    in wr_addr: bit[3]
    in wr_data: bit[WIDTH]
    in wr_enable: bit

    #[detection_signal]
    out single_bit_error: bit
    #[detection_signal]
    out double_bit_error: bit
}

impl EccRegisterFile<'clk, DEPTH, WIDTH> {
    #[retention]
    #[memory(depth = 8, width = 256, style = register)]
    signal registers: [Hash; 8]

    #[retention]
    signal ecc_bits: [bit[5]; 8]
}
```

**`#[safety_mechanism(type = ecc, coverage = 99.9)]`** declares this entity as a safety mechanism with 99.9% diagnostic coverage. This flows into the FMEDA — the fault injection system knows this entity is protective, not functional, and classifies its failure rate as λ_SM.

**`#[detection_signal]`** marks outputs that detect errors. The fault injection system uses these to determine whether injected faults are detected.

**`#[memory(depth = 8, width = 256, style = register)]`** tells synthesis to implement this as register-based storage (not BRAM), which matters for ECC because register-based memories have different failure modes than SRAM.

Sangam's lockstep comparison is another safety pattern — comparing two FPGAs' outputs with debounced mismatch detection:

```
mismatch_detected = (lockstep_rx.phase_shift != phase_limited) ||
                    (lockstep_rx.state != state_reg) ||
                    (lockstep_rx.master_enable != master_enable)

on(clk.rise) {
    if lockstep_rx_valid {
        if mismatch_detected {
            if lockstep_mismatch_count < 10 {
                lockstep_mismatch_count = lockstep_mismatch_count + 1
            } else {
                lockstep_fault = 1
            }
        } else {
            if lockstep_mismatch_count > 0 {
                lockstep_mismatch_count = lockstep_mismatch_count - 1
            }
        }
    }
}
```

Ten consecutive mismatches trigger a lockstep fault. Single-cycle disagreements (noise, slight timing differences) are filtered out by the debounce counter. This is the kind of safety logic that looks simple but gets the details wrong in most first implementations — the saturating counter, the decrement-on-match, the threshold check.

---

## Debug Infrastructure

Karythra uses trace and breakpoint annotations for simulation visibility:

```
#[trace(group = "pipeline_s1", display_name = "S1 Valid")]
signal pipe1_valid: bit

#[trace(group = "pipeline_s1", radix = hex)]
signal pipe1_hash1: Hash

#[breakpoint(is_error = true, name = "R0_WRITE_VIOLATION",
             message = "Attempted write to protected r0 register")]
signal r0_write_attempt: bit
r0_write_attempt = wr_enable && rd_addr == 0
```

**`#[trace]`** groups signals for waveform visualization. The `group` parameter organizes related signals together; `radix` controls display format; `display_name` overrides the signal name in the viewer.

**`#[breakpoint]`** with `is_error = true` halts simulation when the condition is met. The `r0_write_attempt` example catches illegal writes to the zero register — a bug that would silently corrupt state in Verilog but triggers an immediate, named error in skalp simulation.

These annotations have zero cost in synthesis — they're stripped during compilation. But they make simulation debugging dramatically faster because the waveform viewer knows which signals matter and how to display them.

---

## ADC Conversion Functions

A small but important pattern in power electronics: centralizing ADC-to-engineering-unit conversion:

```
pub fn adc_to_mv(adc: AdcRaw, scale: nat[16]) -> MilliVolts {
    return (adc as nat[16]) * scale
}

pub fn adc_to_ma_signed(adc: AdcRaw, scale: nat[16], offset: nat[16]) -> MilliAmps {
    return ((adc as int[16]) - (offset as int[16])) * (scale as int[16])
}
```

Every ADC reading passes through a conversion function before being used in control logic. The conversion is defined once, used everywhere. The type signature enforces that the output is in engineering units (`MilliVolts`, `MilliAmps`), not raw ADC counts.

The alternative — scattering `adc_value * SCALE_FACTOR` throughout the code — leads to inconsistent scaling and makes it hard to change the ADC configuration. Centralizing conversions is standard practice in embedded software; the same discipline applies to HDL.

---

## Lessons from Real Code

A few observations from reading these projects:

**The type system pulls its weight.** Type aliases, fixed-point types, and structs don't add hardware cost, but they catch entire categories of bugs that are invisible in SystemVerilog. When a PI controller takes `q8_8` coefficients and `q16_16` values, you can't accidentally swap them.

**Composition scales.** Both projects use the same `let name = Entity { ports }` pattern from a single comparator up to a 15-instance hierarchy. The pattern doesn't change at scale — a protection system with five sub-instances looks the same as a top-level controller with four major subsystems.

**Generic parameters solve the simulation speed problem.** Production timing constants (100M cycles for a 1-second timeout) make simulation impractical. Parameterizing them and using reduced values in test wrappers is the difference between a 10-second test run and an overnight simulation.

**Match expressions make state machines readable.** Each state is self-contained in its match arm. You can read one arm and understand that state completely. The exhaustiveness check ensures you haven't forgotten a state. Compare this with SystemVerilog `case` statements where a missing `default` silently produces `x`.

**Hardware safety patterns are just careful engineering.** Hysteresis comparators, debounced fault latches, anti-windup integrators, lockstep comparison — none of these are complex individually. The value is in getting them all right simultaneously in a design that composes them correctly. The language helps by making the structure visible and the composition explicit.
