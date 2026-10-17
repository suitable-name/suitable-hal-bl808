# bl808-hal

A `no_std` Hardware Abstraction Layer (HAL) for the Bouffalo Lab BL808 SoC,
targeting the **Sipeed M1s Dock** development board. The crate implements the
`embedded-hal` 1.0 traits for type-safe peripheral access, with an async
(`smol`-family) runtime used for multi-task examples on the M0 core.

---

## Tri-Core Architecture

The BL808 contains three RISC-V cores that boot and run independently:

| Core | ISA      | Width | Primary role on M1s Dock              |
|------|----------|-------|---------------------------------------|
| M0   | E907     | 32-bit | Wireless (WiFi/BT MAC), boot sequencer |
| D0   | C906     | 64-bit | Application / multimedia               |
| LP   | E902     | 32-bit | Low-power sensor hub                   |

**Cargo feature selection.** The HAL compiles for exactly one core at a time.
The features `m0`, `d0`, and `lp` are mutually exclusive — each enables the
corresponding PAC register definitions and interrupt table. There is no default:

```toml
# In your firmware's Cargo.toml:
bl808-hal = { version = "0.1", features = ["d0"] }   # or "m0" / "lp"
```

Building without selecting a core is a compile error. Never enable more than
one core feature in the same compilation unit.

---

## Driver / Peripheral Matrix

Drivers are gated to the core(s) that own the hardware block. The table below
reflects the `#[cfg(feature = ...)]` gates in `crates/bl808-hal/src/lib.rs`.

### System & Clock

| Module       | m0 | d0 | lp | Notes                                  |
|--------------|----|----|----|----------------------------------------|
| `glb`        | ✓  | ✓  | ✓  | GLB clock control, GPIO split          |
| `mmglb`      |    | ✓  |    | MM-domain GLB (D0 multimedia clocks)   |
| `aon`        | ✓  |    |    | Always-On domain                       |
| `hbn`        | ✓  |    |    | Hibernate / low-power control          |
| `pds`        | ✓  | ✓  |    | Power-Down Sleep controller            |
| `efuse`      | ✓  |    |    | OTP eFuse read                         |
| `sec`        | ✓  |    |    | Security engine (AES/SHA/TRNG)         |
| `debug`      | ✓  | ✓  | ✓  | Debug peripheral (ungated)             |
| `misc`       | ✓  | ✓  | ✓  | Miscellaneous system registers         |

### Memory

| Module  | m0 | d0 | lp | Notes                          |
|---------|----|----|----|--------------------------------|
| `flash` | ✓  |    |    | XIP flash control              |
| `psram` |    | ✓  |    | PSRAM / OCRAM controller       |
| `sdh`   | ✓  |    |    | SD/MMC host controller         |
| `lz4d`  | ✓  | ✓  |    | LZ4 hardware decompressor      |

### Connectivity

| Module      | m0 | d0 | lp | Notes                              |
|-------------|----|----|----|------------------------------------|
| `dma`       | ✓  | ✓  |    | DMA controller                     |
| `uart`      | ✓  | ✓  |    | UART (embedded-hal + nb)           |
| `spi`       | ✓  | ✓  |    | SPI controller                     |
| `i2c`       | ✓  | ✓  |    | I2C controller                     |
| `pwm`       | ✓  | ✓  |    | PWM                                |
| `timer`     | ✓  | ✓  |    | Hardware timers                    |
| `emac`      | ✓  |    |    | Ethernet MAC                       |
| `usb`       | ✓  |    |    | USB OTG (device mode, VDMA)        |
| `iso11898`  | ✓  |    |    | CAN / ISO 11898                    |
| `ir`        | ✓  |    |    | IR TX/RX (NEC protocol helper)     |
| `wifi`      | ✓  |    |    | WiFi coexistence + IPC             |
| `ipc`       | ✓  |    |    | Inter-processor communication      |
| `pta`       | ✓  | ✓  |    | PTA coexistence arbiter            |

### GPIO

| Module       | m0 | d0 | lp | Notes                                    |
|--------------|----|----|----|------------------------------------------|
| `gpio`       | ✓  | ✓  | ✓  | Type-safe GPIO (pins 0–45), embedded-hal |
| `interrupts` | ✓  | ✓  | ✓  | Interrupt vector table helpers           |

### Analog

| Module  | m0 | d0 | lp | Notes                        |
|---------|----|----|----|------------------------------|
| `adc`   | ✓  |    |    | SAR ADC                      |
| `gpip`  | ✓  |    |    | General-Purpose ADC interface |
| `gpdac` | ✓  |    |    | General-Purpose DAC (part of the M0 GPIP block) |
| `tsen`  | ✓  | ✓  | ✓  | On-chip temperature sensor    |

### Audio

| Module  | m0 | d0 | lp | Notes                          |
|---------|----|----|----|---------------------------------|
| `audio` | ✓  | ✓  |    | Audio subsystem                |
| `i2s`   | ✓  |    |    | I2S controller (M0 only)       |
| `codec` |    | ✓  |    | Audio codec (D0 only)          |
| `pdm`   | ✓  | ✓  | ✓  | PDM microphone (ungated)       |
| `agc`   | ✓  | ✓  | ✓  | Automatic Gain Control (ungated)|

### Video & AI

| Module  | m0 | d0 | lp | Notes                              |
|---------|----|----|----|-------------------------------------|
| `dvp`   |    | ✓  |    | DVP camera interface (D0 only)     |
| `dsi`   |    | ✓  |    | MIPI DSI display (D0 only)         |
| `mipi`  |    | ✓  |    | MIPI CSI/DSI PHY (D0 only)         |
| `mjpeg` |    | ✓  |    | MJPEG hardware encoder (D0 only)   |
| `h264`  |    | ✓  |    | H.264 encoder (D0 only)            |
| `npu`   |    | ✓  |    | Neural processing unit (D0 only)   |
| `isp`   |    | ✓  |    | Image signal processor (D0 only)   |
| `osd`   | ✓  | ✓  | ✓  | On-screen display (ungated)        |
| `cci`   | ✓  |    |    | Camera control interface (M0)      |
| `nec`   | ✓  | ✓  | ✓  | NEC IR protocol helper (ungated)   |
| `common`| ✓  | ✓  | ✓  | Shared types (ungated)             |

---

## Building the Library

Select exactly one core feature. All three must be buildable independently.

```sh
# Build for each core
cargo build --features m0
cargo build --features d0
cargo build --features lp
```

### Host Unit Tests

The HAL ships with unit tests that run on the host using mocked register
blocks — no hardware required. Tests use the `critical-section` std backend
and `std::thread_local` GLB mocks so each test thread is isolated.

```sh
cargo test --features m0
cargo test --features d0
cargo test --features lp
```

### Clippy

```sh
cargo clippy --features m0 --tests
cargo clippy --features d0 --tests
cargo clippy --features lp --tests
```

---

## Status / Maturity

- Register access is derived from the Bouffalo Lab PAC (`bl808-pac`), generated
  from the official SVD and cross-checked against the bundled C++ SDK source
  trees (`bl808-pac/M1s_BL808_SDK/` and `bl808-pac/bl_mcu_sdk_bl808/`).
- All host unit tests pass (`cargo test --features <core>`).
- **Not yet hardware-validated on silicon.** Firmware has not been flashed to
  a physical M1s Dock and confirmed to run. Treat this as a bring-up crate.

---

## Repository Layout

```
bl808-hal/
├── Cargo.toml               # workspace root (excludes bl808-pac and apps)
├── crates/
│   └── bl808-hal/           # HAL library crate
│       ├── src/             # driver modules (one subdirectory per peripheral)
│       └── tests/           # integration test stubs (unit tests live in src/)
├── bl808-pac/               # read-only submodule: PAC + bundled C++ SDKs
│   ├── src/                 # generated PAC Rust source
│   ├── M1s_BL808_SDK/       # Sipeed M1s BL808 C SDK
│   └── bl_mcu_sdk_bl808/    # Bouffalo Lab MCU SDK (BL602/BL702/BL808)
└── apps/
    ├── blinky_d0/           # D0 (C906) LED blink — see apps/blinky_d0/README.md
    └── wifi_m0/             # M0 (E907) WiFi/IPC demo — see apps/wifi_m0/README.md
```

`apps/` and `bl808-pac/` are excluded from the workspace so that the HAL
library can be tested independently with `cargo test --features <core>`.

---

## Examples

- [`apps/blinky_d0/README.md`](apps/blinky_d0/README.md) — D0 LED blink
- [`apps/wifi_m0/README.md`](apps/wifi_m0/README.md) — M0 WiFi coexistence + IPC
