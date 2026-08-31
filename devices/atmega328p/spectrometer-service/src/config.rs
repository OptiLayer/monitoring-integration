use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::data_source::DataSourceConfig;
use crate::processing::outlier::OutlierMethod;

#[derive(Parser, Debug)]
#[command(name = "spectrometer-service")]
#[command(about = "ATmega328P Monochromatic Spectrometer Service")]
#[command(version)]
pub struct Cli {
    /// HTTP server port
    #[arg(short, long, default_value = "8100")]
    pub listen: u16,

    /// HTTP server host
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,

    /// List available serial ports and exit
    #[arg(long)]
    pub list_ports: bool,

    /// Outlier exclusion method
    #[arg(long, value_enum, default_value = "grubbs")]
    pub outlier_method: OutlierMethodArg,

    /// Alpha value for Grubbs test (significance level)
    #[arg(long, default_value = "0.05")]
    pub grubbs_alpha: f64,

    /// Enable monochromator control: `sim`, or the folder holding the Solar LS
    /// `InstrumentCfg*.xml` (empty string = next to the executable, Windows only)
    #[arg(long)]
    pub mono: Option<String>,

    /// Monochromator instrument index within the SDK config
    #[arg(long, default_value = "0")]
    pub mono_index: i32,

    /// Pin the monochromator to one grating instead of auto-selecting per wavelength
    #[arg(long)]
    pub mono_grating: Option<i32>,

    /// Path to calibration config file
    #[arg(long, default_value = "calibration.toml")]
    pub calibration_config: std::path::PathBuf,

    #[command(subcommand)]
    pub mode: Option<Mode>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Mode {
    /// Connect to real hardware via serial port
    Serial(SerialArgs),

    /// Playback from log file
    Playback(PlaybackArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SerialArgs {
    /// Serial port device path (e.g., COM3 on Windows, /dev/ttyUSB0 on Linux)
    #[arg(short, long)]
    pub device: String,

    /// Baud rate
    #[arg(short, long, default_value = "38400")]
    pub baud: u32,

    /// ADC gain setting (1, 2, 4, 8, 16, 32, 64, 128). Overrides saved config.
    #[arg(long)]
    pub gain: Option<u8>,

    /// ADC sample rate in Hz. Overrides saved config.
    #[arg(long)]
    pub fadc: Option<f32>,

    /// Dump raw serial output to file for later playback
    #[arg(long)]
    pub log_file: Option<std::path::PathBuf>,

    /// Number of measurements per series (1-12). Overrides saved config.
    #[arg(long)]
    pub count: Option<u8>,
}

#[derive(Args, Debug, Clone)]
pub struct PlaybackArgs {
    /// Path to log file (supports both timestamped and raw serial log formats)
    #[arg(short, long)]
    pub file: PathBuf,

    /// Playback speed multiplier (1.0 = real-time, 2.0 = 2x speed)
    #[arg(short, long, default_value = "1.0")]
    pub speed: f64,

    /// Loop playback when file ends
    #[arg(long, default_value = "false")]
    pub loop_playback: bool,

    /// Cycle interval in ms for raw logs without timestamps (default: 100)
    #[arg(long, default_value = "100")]
    pub cycle_interval: u64,
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum OutlierMethodArg {
    /// No outlier exclusion
    None,
    /// Grubbs' test (default)
    #[default]
    Grubbs,
}

impl Cli {
    /// Convert CLI args to DataSourceConfig.
    /// For serial mode, CLI args override saved config; saved config overrides hardcoded defaults.
    pub fn to_data_source_config(
        &self,
        saved: &crate::service::calibration::DeviceSettings,
    ) -> Option<DataSourceConfig> {
        match &self.mode {
            Some(Mode::Serial(args)) => Some(DataSourceConfig::Serial {
                port: args.device.clone(),
                baud_rate: args.baud,
                gain: args.gain.unwrap_or(saved.gain),
                fadc: args.fadc.unwrap_or(saved.fadc),
                count: args.count.unwrap_or(saved.count),
                log_file: args.log_file.clone(),
            }),
            Some(Mode::Playback(args)) => Some(DataSourceConfig::Playback {
                log_file: args.file.clone(),
                speed_multiplier: args.speed,
                loop_playback: args.loop_playback,
                cycle_interval_ms: args.cycle_interval,
            }),
            None => None,
        }
    }

    /// Convert CLI args to OutlierMethod
    pub fn to_outlier_method(&self) -> OutlierMethod {
        match self.outlier_method {
            OutlierMethodArg::None => OutlierMethod::None,
            OutlierMethodArg::Grubbs => OutlierMethod::Grubbs {
                alpha: self.grubbs_alpha,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_serial() {
        let cli = Cli::parse_from([
            "spectrometer-service",
            "--listen",
            "8200",
            "serial",
            "--device",
            "COM3",
        ]);

        assert_eq!(cli.listen, 8200);
        assert!(matches!(cli.mode, Some(Mode::Serial(_))));

        if let Some(Mode::Serial(args)) = cli.mode {
            assert_eq!(args.device, "COM3");
            assert_eq!(args.baud, 38400);
        }
    }

    #[test]
    fn test_cli_parse_playback() {
        let cli = Cli::parse_from([
            "spectrometer-service",
            "playback",
            "--file",
            "test.log",
            "--speed",
            "2.0",
            "--loop-playback",
        ]);

        assert!(matches!(cli.mode, Some(Mode::Playback(_))));

        if let Some(Mode::Playback(args)) = cli.mode {
            assert_eq!(args.file, PathBuf::from("test.log"));
            assert_eq!(args.speed, 2.0);
            assert!(args.loop_playback);
        }
    }

    #[test]
    fn test_cli_parse_list_ports() {
        let cli = Cli::parse_from(["spectrometer-service", "--list-ports"]);

        assert!(cli.list_ports);
    }

    #[test]
    fn test_to_outlier_method() {
        let cli = Cli::parse_from(["spectrometer-service", "--outlier-method", "none"]);
        assert!(matches!(cli.to_outlier_method(), OutlierMethod::None));

        let cli = Cli::parse_from([
            "spectrometer-service",
            "--outlier-method",
            "grubbs",
            "--grubbs-alpha",
            "0.01",
        ]);
        if let OutlierMethod::Grubbs { alpha } = cli.to_outlier_method() {
            assert!((alpha - 0.01).abs() < 0.001);
        } else {
            panic!("Expected Grubbs method");
        }
    }

    #[test]
    fn test_to_data_source_config_with_cli_overrides() {
        use crate::service::calibration::DeviceSettings;

        let cli = Cli::parse_from([
            "spectrometer-service",
            "serial",
            "--device",
            "/dev/ttyUSB0",
            "--baud",
            "115200",
            "--gain",
            "8",
            "--fadc",
            "500",
            "--count",
            "7",
        ]);

        let saved = DeviceSettings::default(); // gain=2, fadc=250, count=4
        let config = cli.to_data_source_config(&saved);
        assert!(config.is_some());

        if let Some(DataSourceConfig::Serial {
            port,
            baud_rate,
            gain,
            fadc,
            count,
            ..
        }) = config
        {
            assert_eq!(port, "/dev/ttyUSB0");
            assert_eq!(baud_rate, 115200);
            // CLI overrides saved config
            assert_eq!(gain, 8);
            assert_eq!(fadc, 500.0);
            assert_eq!(count, 7);
        } else {
            panic!("Expected Serial config");
        }
    }

    #[test]
    fn test_to_data_source_config_falls_back_to_saved() {
        use crate::service::calibration::DeviceSettings;

        let cli = Cli::parse_from(["spectrometer-service", "serial", "--device", "/dev/ttyUSB0"]);

        let saved = DeviceSettings {
            gain: 4,
            fadc: 500.0,
            count: 3,
            ..DeviceSettings::default()
        };
        let config = cli.to_data_source_config(&saved);

        if let Some(DataSourceConfig::Serial {
            gain, fadc, count, ..
        }) = config
        {
            // Falls back to saved config values
            assert_eq!(gain, 4);
            assert_eq!(fadc, 500.0);
            assert_eq!(count, 3);
        } else {
            panic!("Expected Serial config");
        }
    }
}
