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

**In SystemVerilog,** there are no type aliases for signals. You'd write:

```systemverilog
logic [15:0] v_bat_mv;   // millivolts? raw ADC? who knows
logic [15:0] i_bat_ma;   // hope the comment is accurate
logic [31:0] int_accum;  // Q16.16? Q8.24? check the comments
```

Comments are the only mechanism for encoding units and fixed-point format, and comments drift from reality. There's no `typedef` that carries semantic meaning through the type checker — `typedef logic [15:0] millivolts_t` creates a type alias, but SystemVerilog won't warn you if you assign a `millivolts_t` to an `milliamps_t`. It's the same 16 bits. skalp's type aliases are equally transparent to the compiler today, but they make code review catch errors that SystemVerilog hides in identical `logic [N:0]` declarations.

The fixed-point situation is worse. SystemVerilog has no fixed-point type at all. You track the Q format in comments and manually insert shifts after every multiply. Forget one shift and your integrator value is off by 256x — a bug that compiles clean and only shows up when the motor oscillates.

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

**In SystemVerilog,** state machines use `case` statements with `enum` types:

```systemverilog
typedef enum logic [3:0] {
    INIT, WAIT_BMS, PRECHARGE, SOFT_START,
    RUNNING, STANDBY, DERATE, FAULT, SHUTDOWN
} module_state_t;

always_ff @(posedge clk) begin
    if (rst) state <= INIT;
    else begin
        case (state)
            INIT: begin
                master_enable <= 0;
                if (enable && !any_fault) state <= WAIT_BMS;
            end
            WAIT_BMS: begin
                if (!enable || any_fault) state <= FAULT;
                else if (bms_connected) state <= PRECHARGE;
            end
            // ...
            default: state <= INIT;  // what if you forget this?
        endcase
    end
end
```

The structure is similar, but there are two key differences. First, SystemVerilog's `case` doesn't check exhaustiveness — if you add a tenth state to the enum but forget to add a case arm, the `default` branch handles it silently. In skalp, a `match` without full coverage is a compile error. You can't add `ModuleState::Emergency` without updating every `match` that operates on the state.

Second, SystemVerilog's `default` branch is a trap. It looks safe — "catch everything else" — but it masks exactly the bugs you want to find: unhandled states. In safety-critical designs, reaching an unhandled state should be an explicit error, not a silent reset to `INIT`. skalp forces you to handle every case or explicitly write a catch-all, making the decision visible.

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

**In SystemVerilog,** module instantiation supports both positional and named port mapping:

```systemverilog
// Named (safe but verbose)
protection_system u_protection (
    .clk        (clk),
    .rst        (rst),
    .voltage    (v_bat_mv),
    .current    (i_bat_ma),
    .faults     (prot_faults),
    .any_hard_fault (prot_hard)
    // forgot .any_soft_fault — no error, it's unconnected
);

// Positional (compact but fragile)
protection_system u_protection (
    clk, rst, v_bat_mv, i_bat_ma, ...
    // reorder a port in the module definition and this silently breaks
);
```

SystemVerilog's named mapping (`.name(signal)`) is equivalent in safety to skalp's syntax. The real difference is what happens with unconnected ports. In SystemVerilog, an output port you forget to connect is silently unconnected — no warning by default. An input port left unconnected gets `z` (high impedance), which propagates as `x` through any logic it touches. Some lint tools catch this, but the language doesn't.

In skalp, all ports must be connected at instantiation. An unconnected output requires an explicit `_` to acknowledge the omission. Forgetting a port is a compile error, not a latent `x` propagation bug.

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

**In SystemVerilog,** you can use `struct` types, but they're rarely used for module ports in practice:

```systemverilog
typedef struct packed {
    logic ov, uv, oc, ot, desat, bms_fault, bms_timeout, lockstep;
} fault_flags_t;

// Works as a port, but...
module dab_controller (
    output fault_flags_t faults
);
```

SystemVerilog structs exist, and `packed` structs can even be used as ports. But in practice, most teams avoid struct ports because of tool compatibility issues — some synthesis tools handle struct ports poorly, and mixing tools (one vendor's synthesis, another's simulation) can produce different struct layouts. The result is that most production SystemVerilog uses flat `logic` ports and passes around bit vectors, reassembling them with `assign` statements.

skalp structs are always flattened to individual signals during MIR lowering, so they're guaranteed to synthesize correctly. The struct is a purely compile-time grouping mechanism — the synthesis tool never sees it.

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

**In SystemVerilog,** parameterized modules work similarly:

```systemverilog
module dab_controller #(
    parameter SOFT_START_CYCLES = 1_000_000,
    parameter BMS_TIMEOUT_CYCLES = 100_000_000
) ( ... );
```

SystemVerilog parameters are actually quite capable here — the basic mechanism is the same. The difference is in what you can compute with them. skalp's const generics support full expression evaluation at compile time: `clog2(DEPTH + 1)`, complex type arithmetic, const functions. SystemVerilog's `$clog2` works but its constant expression evaluation is more limited and the `localparam` boilerplate adds up:

```systemverilog
localparam ADDR_WIDTH = $clog2(DEPTH);
localparam PTR_WIDTH = $clog2(DEPTH + 1);  // one extra bit for full/empty
logic [ADDR_WIDTH-1:0] wr_ptr;             // don't forget the -1
logic [PTR_WIDTH-1:0] count;               // different -1 here
```

In skalp, `signal wr_ptr: nat[clog2(DEPTH)]` handles the width automatically — no `-1`, no `localparam`, no opportunity for off-by-one errors.

The bigger difference is generic *functions*. SystemVerilog has parameterized modules but no parameterized functions. Karythra's `exec_l0_l1::<WORD_SIZE>(...)` is a compile-time specialized function. In SystemVerilog, you'd either write separate functions for each width or use a `generate` block inside a module — which means wrapping a function in a module just to parameterize it.

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

**In SystemVerilog,** the PI controller is roughly the same length, but several patterns don't translate:

```systemverilog
// No forward references — saturated must be defined BEFORE it's used
wire saturated = (sum_raw > out_max) || (sum_raw < out_min);

always_ff @(posedge clk) begin
    if (rst) int_accum <= 0;
    else if (enable && update) begin
        // Anti-windup logic — same structure
        if (!saturated || (saturated && sum_raw > out_max && error < 0) ||
            (saturated && sum_raw < out_min && error > 0)) begin
            int_accum <= int_accum + ((ki * error) >>> 8);
        end
    end
end

// Fixed-point: hope you remembered the shift
assign prop_term = (kp * error) >>> 8;  // arithmetic right shift, not >>
assign sum_raw = prop_term + int_accum;
assign output = (sum_raw > out_max) ? out_max :
                (sum_raw < out_min) ? out_min : sum_raw;
```

Three differences matter. First, SystemVerilog requires `wire`/`assign` declarations before use — no forward references. The `saturated` signal must be declared above the `always_ff` block where it's used. In skalp, combinational signals can appear in any order because they have no temporal dependency. This lets you group related logic together (sequential update near its combinational output) rather than ordering by dependency.

Second, fixed-point arithmetic is entirely manual. `>>>` (arithmetic right shift) preserves the sign bit; `>>` (logical right shift) does not. Using `>>` instead of `>>>` for a signed Q16.16 value introduces a sign bug that compiles without warning. skalp's `fixed<32, 16, true>` type makes the signedness explicit in the type, and the `>> 8` operation knows whether to do arithmetic or logical shift based on the type.

Third, the cascaded instantiation (`outer_loop.output` feeding `inner_loop.setpoint`) requires intermediate `wire` declarations in SystemVerilog. In skalp, `outer_loop.output` is a direct expression you can use inline.

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

**In SystemVerilog,** the comparator and latch logic is structurally identical — this is standard digital design. The difference is in composition. Instantiating five comparators and five latches in SystemVerilog means 10 module instantiations with positional or named ports. Wrapping them in a `protection_system` module means writing the module, its port list, internal wires for every inter-module connection, and the 10 instantiations. It works, but the boilerplate-to-logic ratio is high.

The more interesting difference is that SystemVerilog has no mechanism for the `ProtectionThresholds` struct to flow through the hierarchy as a single port. You'd either pass individual threshold values (5 modules × 2 thresholds = 10 ports) or use a packed struct port (which some synthesis tools handle poorly). skalp's struct ports flatten at compile time, so the synthesis tool sees individual signals but the source code sees grouped configuration.

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

**In SystemVerilog,** the parallel-compute-and-mux pattern is identical — this is just hardware design:

```systemverilog
wire [63:0] l0_l1_result = exec_l0_l1(function_sel, data1, data2);
wire [63:0] l2_result    = exec_l2(function_sel, data1, data2);
wire [63:0] l3_result    = exec_l3(function_sel, data1, data2);
wire [63:0] l4_l5_result = exec_l4_l5(function_sel, data1, data2);

assign fu_result = (function_sel < 18) ? l0_l1_result :
                   (function_sel < 28) ? l2_result :
                   (function_sel < 38) ? l3_result : l4_l5_result;
```

No real difference here. skalp's `if-else` expressions and SystemVerilog's ternary chains produce the same hardware. The skalp version is arguably more readable for deeply nested selections, but both are clear enough.

The difference is the `#[power_domain]` annotation. SystemVerilog has no concept of power domains in the language — power intent is described in a separate UPF (Unified Power Format) file, maintained by a different team, using a different tool. In skalp, `#[power_domain(id = 1, wake_cycles = 4)]` lives on the entity itself, so the power architecture is visible in the source code and flows through to synthesis automatically.

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

**In SystemVerilog,** the shadow register pattern is identical. This is a pure hardware design pattern — skalp doesn't add anything over SystemVerilog here. Two registers, one loads continuously, one swaps on trigger. The RTL is the same in any language.

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

**In SystemVerilog,** this pattern is impossible. There is no async circuit support in SystemVerilog — the language assumes synchronous design throughout. If you wanted an NCL version of the same function unit, you'd hand-instantiate dual-rail signals, threshold gates, and completion detection in structural Verilog. The synchronous and async versions would share no code. Every change to the computational logic would require updating both implementations manually and hoping they stay in sync.

This is perhaps the starkest difference between the two languages. skalp's `async entity` keyword and compiler-managed NCL expansion give you two implementations from one source. SystemVerilog gives you no path to async circuits at all.

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

**In SystemVerilog,** safety is entirely outside the language. There are no safety annotations — ECC protection, diagnostic coverage, detection signals, and FMEDA classification are tracked in external documents (usually Excel spreadsheets). The connection between a module's RTL and its row in the FMEDA is maintained by convention: someone writes "this module has ECC" in a spreadsheet cell and hopes it stays true.

skalp's `#[safety_mechanism]` and `#[detection_signal]` annotations are machine-readable. The fault injection system uses them to classify failure rates, identify detection signals, and generate FMEDA entries automatically. When you change the design, the safety analysis updates with it. In SystemVerilog, the design and the safety documentation are separate artifacts that drift apart.

Similarly, `#[memory(style = register)]` is a synthesis hint that SystemVerilog handles with tool-specific pragmas (`(* ram_style = "distributed" *)` for Xilinx, `/* synthesis ramstyle = "logic" */` for Intel). Each vendor's pragma is different, and they're stringly-typed — a typo is silently ignored.

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

**In SystemVerilog,** the equivalent debug infrastructure is scattered across multiple mechanisms:

```systemverilog
// Waveform grouping: done in the simulator GUI, not in the source
// You manually drag signals into groups every time you open the waveform

// Breakpoints: SystemVerilog assertions
assert property (@(posedge clk) !(wr_enable && rd_addr == 0))
    else $error("R0_WRITE_VIOLATION: Attempted write to protected r0 register");
```

SystemVerilog assertions (SVA) are powerful for the breakpoint use case — `assert property` can express complex temporal conditions. But waveform organization has no language support at all. Every engineer manually configures their waveform viewer, creating `.do` files or `.gtkw` scripts that aren't checked into version control and aren't portable between tools.

skalp's `#[trace(group = "pipeline_s1")]` puts waveform organization in the source code. The debug setup travels with the design, works for every engineer, and doesn't depend on a specific simulator's UI.

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

**In SystemVerilog,** you'd use `function` for the same purpose:

```systemverilog
function automatic logic [15:0] adc_to_mv(input logic [11:0] adc, input logic [15:0] scale);
    return adc * scale;
endfunction
```

This works — SystemVerilog functions are synthesizable. The main difference is that skalp's typed return value (`-> MilliVolts`) makes the unit conversion visible in the signature. SystemVerilog's `logic [15:0]` return type tells you nothing about what the value represents. The function could return millivolts, raw ADC counts, or a temperature — the type is the same 16 bits.

This is a case where both languages support the pattern, but skalp's type aliases add documentation value that SystemVerilog can't express.

---

## Lessons from Real Code

A few observations from reading these projects:

**Not everything is better — and that's fine.** Shadow registers, parallel-compute-and-mux, pipeline valid flags — some patterns are pure hardware design and look the same in any language. skalp doesn't pretend to improve what doesn't need improving.

**Where skalp helps is in the gaps.** The patterns where SystemVerilog provides no support at all: async/NCL circuits from the same source, safety annotations that flow into FMEDA, power domain intent in the source code, waveform organization in the design files, forward references for grouping related logic, struct ports that reliably flatten during synthesis.

**Where skalp nudges is in the details.** Exhaustive match instead of `default`-masked `case`. Typed fixed-point instead of manual shift tracking. Mandatory port connections instead of silent `z`. Type aliases that make domain units visible. These aren't revolutionary individually, but they compound — each one prevents a class of bug that in SystemVerilog survives compilation and surfaces in hardware.

**The hardware design patterns are the same.** PI controllers, state machines, fault latches, protection hierarchies — these are domain patterns, not language patterns. A good power electronics engineer writes the same anti-windup logic in any language. What changes is how many opportunities the language gives you to get the *boilerplate* wrong while the *algorithm* is right.

**Composition and parameterization are where the biggest time savings come from.** Not from individual language features, but from the ease of wrapping a tested entity in a new context. Sangam's test wrapper — the same controller with shorter timeouts — is one line of generic parameters. Karythra's sync-to-async port — the same function units in an `async entity` — is a keyword change. These are the patterns that prevent copy-paste divergence in production codebases.
