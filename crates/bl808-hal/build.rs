use std::{env, fs::File, io::Write, path::PathBuf};

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut f = File::create(out.join("device.x")).unwrap();

    // NOTE: In build.rs, we must check env vars for features, not use #[cfg(feature=...)]
    // because #[cfg] applies to the build script compilation, not the crate compilation.
    let has_d0 = env::var("CARGO_FEATURE_D0").is_ok();
    let has_m0 = env::var("CARGO_FEATURE_M0").is_ok();

    if has_d0 {
        let interrupts = vec![
            "SSIP0",
            "SSIP1",
            "SSIP2",
            "SSIP3",
            "UART3",
            "I2C2",
            "I2C3",
            "SPI1",
            "SEC_ENG_ID1",
            "SEC_ENG_ID0",
            "SF_CTRL",
            "I2S1",
            "DVP2_BUS",
            "DVP2_INT",
            "MJPEG",
            "DSI",
            "CSI",
            "GPDAC",
            "CSI_INT",
            "USB",
            "SDH",
            "MM_MISC",
            "DMA2D_INT0",
            "DMA2D_INT1",
            "DMA2_INT0",
            "DMA2_INT1",
            "DMA2_INT2",
            "DMA2_INT3",
            "DMA2_INT4",
            "DMA2_INT5",
            "DMA2_INT6",
            "DMA2_INT7",
            "PD_HBN_OUT0",
            "PD_HBN_OUT1",
            "PDS_WAKEUP",
            "FIFO_ERR",
            "EMAC",
            "WL_ALL",
            "BT_ALL",
            "RF_TOP_INT0",
            "RF_TOP_INT1",
            "IPC_M0",
            "AUDIO",
            "SEC_ENG_ID2",
            "GPIO_INT0",
            "DMA2D_INT2",
            "DMA2D_INT3",
            "PLIC_SGI",
            "PWM1",
            "TIMER1_CH0",
            "TIMER1_CH1",
            "TIMER1_WDT",
        ];
        for name in interrupts {
            writeln!(f, "PROVIDE({} = DefaultHandler);", name).unwrap();
        }
    }

    if has_m0 {
        let interrupts = vec![
            "SSIP0",
            "SSIP1",
            "SSIP2",
            "SSIP3",
            "SBIP0",
            "SBIP1",
            "SBIP2",
            "SBIP3",
            "PDS",
            "HBN",
            "PWR",
            "SEC_ENG",
            "SF_CTRL",
            "DVP",
            "EMAC_RSV",
            "DMA0_ALL",
            "DMA1_ALL",
            "DMA2_INT0",
            "DMA2_INT1",
            "IRTX",
            "IRRX",
            "USB",
            "AUPDM",
            "RESERVED_23",
            "EMAC",
            "GPADC_DMA",
            "SDH",
            "SPI0",
            "UART0",
            "UART1",
            "UART2",
            "GPIO_DMA",
            "I2C0",
            "PWM0",
            "IPC_D0",
            "AUPLL",
            "TIMER0_CH0",
            "TIMER0_CH1",
            "TIMER0_WDT",
            "I2C1",
            "I2S",
            "ANA",
            "BOOT",
            "SEC_ENG_ID1",
            "GPIO_INT0",
            "DMA2D_INT0",
            "DMA2D_INT1",
            "MJPEG",
            "PWM1",
            "UART3",
            "PDS_WAKEUP",
            "HBN_OUT0",
            "HBN_OUT1",
            "WIFI_IPC",
            "BT_IPC",
            "TB_ALL",
            "BT_ALL",
            "RF_TOP_INT0",
            "RF_TOP_INT1",
            "FIFO_ERR",
            "DMA2_INT2",
            "DMA2_INT3",
            "DMA2_INT4",
            "DMA2_INT5",
        ];

        for name in interrupts {
            writeln!(f, "PROVIDE({} = DefaultHandler);", name).unwrap();
        }
    }

    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
}
