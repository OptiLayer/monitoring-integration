//! 242-byte LZH frame codec.
//!
//! Two directions with asymmetric encodings:
//! * `OutFrame` — SPA→PCS, framed with `0x5AA5` magic at bytes 0..2 and 240..242,
//!   bool encoding `0x05` = true, `0x01` = false.
//! * `InFrame` — PCS→SPA, no magic, mostly zeros; bool encoding `0x01` = true,
//!   `0x00` = false. Only three bool fields ever vary in observed traffic.
//!
//! All multi-byte numbers are little-endian.

use thiserror::Error;

pub const FRAME_LEN: usize = 242;

const MAGIC: [u8; 2] = [0x5A, 0xA5];

const OUT_BOOL_TRUE: u8 = 0x05;
const OUT_BOOL_FALSE: u8 = 0x01;
const IN_BOOL_TRUE: u8 = 0x01;

const BYTE10_CONST: u8 = 0x01;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame length must be {FRAME_LEN}, got {0}")]
    BadLength(usize),
    #[error("bad header magic: expected 5AA5, got {0:02X}{1:02X}")]
    BadHeader(u8, u8),
    #[error("bad footer magic: expected 5AA5, got {0:02X}{1:02X}")]
    BadFooter(u8, u8),
}

/// SPA→PCS frame (the direction our service emits).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct OutFrame {
    pub heartbeat: u16,
    pub ready: bool,
    pub terminate_layer: bool,
    pub fault_output: bool,
    pub deposit: bool,
    pub continue_: bool,
    pub fault_input: bool,
    pub acp: bool,
    pub layer: u16,
    pub auto_scaling: bool,
    pub current_test_glass: u16,
    pub number_layers: u16,
    pub coating_finished: bool,
    pub current_thickness: f64,
    pub remaining_time: f64,
    pub current_rate: f64,
    pub design_thickness: f64,
    pub design_rate: f64,
    pub mean_rate: f64,
    pub sample_position_counts: f64,
    pub central_wavelength: f64,
    pub n_index: u16,
}

impl OutFrame {
    pub fn encode(&self) -> [u8; FRAME_LEN] {
        let mut buf = [0u8; FRAME_LEN];
        buf[0..2].copy_from_slice(&MAGIC);
        buf[10] = BYTE10_CONST;
        buf[14..16].copy_from_slice(&self.heartbeat.to_le_bytes());
        buf[16] = out_bool(self.ready);
        buf[18] = out_bool(self.terminate_layer);
        buf[20] = out_bool(self.fault_output);
        buf[22] = out_bool(self.deposit);
        buf[24] = out_bool(self.continue_);
        buf[26] = out_bool(self.fault_input);
        buf[28] = out_bool(self.acp);
        buf[30..32].copy_from_slice(&self.layer.to_le_bytes());
        buf[32] = out_bool(self.auto_scaling);
        buf[34..36].copy_from_slice(&self.current_test_glass.to_le_bytes());
        buf[36..38].copy_from_slice(&self.number_layers.to_le_bytes());
        buf[38] = out_bool(self.coating_finished);
        buf[40..48].copy_from_slice(&self.current_thickness.to_le_bytes());
        buf[48..56].copy_from_slice(&self.remaining_time.to_le_bytes());
        buf[56..64].copy_from_slice(&self.current_rate.to_le_bytes());
        buf[64..72].copy_from_slice(&self.design_thickness.to_le_bytes());
        buf[72..80].copy_from_slice(&self.design_rate.to_le_bytes());
        buf[80..88].copy_from_slice(&self.mean_rate.to_le_bytes());
        buf[88..96].copy_from_slice(&self.sample_position_counts.to_le_bytes());
        buf[96..104].copy_from_slice(&self.central_wavelength.to_le_bytes());
        buf[104..106].copy_from_slice(&self.n_index.to_le_bytes());
        buf[240..242].copy_from_slice(&MAGIC);
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, FrameError> {
        if buf.len() != FRAME_LEN {
            return Err(FrameError::BadLength(buf.len()));
        }
        if buf[0..2] != MAGIC {
            return Err(FrameError::BadHeader(buf[0], buf[1]));
        }
        if buf[240..242] != MAGIC {
            return Err(FrameError::BadFooter(buf[240], buf[241]));
        }
        Ok(Self {
            heartbeat: u16::from_le_bytes([buf[14], buf[15]]),
            ready: in_bool_out(buf[16]),
            terminate_layer: in_bool_out(buf[18]),
            fault_output: in_bool_out(buf[20]),
            deposit: in_bool_out(buf[22]),
            continue_: in_bool_out(buf[24]),
            fault_input: in_bool_out(buf[26]),
            acp: in_bool_out(buf[28]),
            layer: u16::from_le_bytes([buf[30], buf[31]]),
            auto_scaling: in_bool_out(buf[32]),
            current_test_glass: u16::from_le_bytes([buf[34], buf[35]]),
            number_layers: u16::from_le_bytes([buf[36], buf[37]]),
            coating_finished: in_bool_out(buf[38]),
            current_thickness: read_f64(&buf[40..48]),
            remaining_time: read_f64(&buf[48..56]),
            current_rate: read_f64(&buf[56..64]),
            design_thickness: read_f64(&buf[64..72]),
            design_rate: read_f64(&buf[72..80]),
            mean_rate: read_f64(&buf[80..88]),
            sample_position_counts: read_f64(&buf[88..96]),
            central_wavelength: read_f64(&buf[96..104]),
            n_index: u16::from_le_bytes([buf[104], buf[105]]),
        })
    }
}

/// PCS→SPA frame (the direction our service receives).
///
/// The customer's PCS sends 242-byte frames with no header magic and almost
/// all bytes zero. Across a 32-minute deposition only the three control bools
/// below ever changed value. Everything else is ignored.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InFrame {
    pub deposit: bool,
    pub acp: bool,
    pub auto_scaling: bool,
}

impl InFrame {
    pub fn decode(buf: &[u8]) -> Result<Self, FrameError> {
        if buf.len() != FRAME_LEN {
            return Err(FrameError::BadLength(buf.len()));
        }
        Ok(Self {
            deposit: buf[118] == IN_BOOL_TRUE,
            acp: buf[124] == IN_BOOL_TRUE,
            auto_scaling: buf[126] == IN_BOOL_TRUE,
        })
    }

    pub fn encode(&self) -> [u8; FRAME_LEN] {
        let mut buf = [0u8; FRAME_LEN];
        buf[118] = in_bool(self.deposit);
        buf[124] = in_bool(self.acp);
        buf[126] = in_bool(self.auto_scaling);
        buf
    }
}

fn out_bool(b: bool) -> u8 {
    if b { OUT_BOOL_TRUE } else { OUT_BOOL_FALSE }
}

fn in_bool(b: bool) -> u8 {
    if b { IN_BOOL_TRUE } else { 0 }
}

/// Decode an outbound-direction bool. `0x05` = true, anything else = false.
/// Matches the Delphi `MtClient.ExtractBool` semantics observed on the wire.
fn in_bool_out(byte: u8) -> bool {
    byte == OUT_BOOL_TRUE
}

fn read_f64(slice: &[u8]) -> f64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(slice);
    f64::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPA→PCS frame captured at t=1947.31s in `deposition.pcapng` (frame
    /// #125731) — the TerminateLayer pulse that ends layer 1. All major fields
    /// are non-zero, making this the highest-signal anchor we have.
    const TERMINATE_LAYER_FRAME_HEX: &str = "5aa5000000000000000001000000641e0500050001000100010001000500010001000100010001001b30b1bbb71b72401813ff1753230840fc0e3c889d04d13f00000000003072409a9999999999c93ffc0e3c889d04d13f00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005aa5";

    fn hex_to_frame(hex: &str) -> [u8; FRAME_LEN] {
        let cleaned: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = (0..cleaned.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), FRAME_LEN, "fixture must be exactly 242 bytes");
        let mut buf = [0u8; FRAME_LEN];
        buf.copy_from_slice(&bytes);
        buf
    }

    #[test]
    fn out_frame_decodes_real_capture() {
        let buf = hex_to_frame(TERMINATE_LAYER_FRAME_HEX);
        let f = OutFrame::decode(&buf).unwrap();
        assert_eq!(f.heartbeat, 7780);
        assert!(f.ready);
        assert!(f.terminate_layer);
        assert!(!f.deposit);
        assert!(f.acp);
        assert_eq!(f.layer, 1);
        assert_eq!(f.current_test_glass, 1);
        assert_eq!(f.number_layers, 1);
        assert!(!f.coating_finished);
        assert_eq!(f.design_thickness, 291.0);
        assert_eq!(f.design_rate, 0.2);
        assert!((f.current_thickness - 289.732_356_731_548_6).abs() < 1e-9);
        assert!((f.current_rate - 0.265_906_699_220_280_76).abs() < 1e-9);
    }

    #[test]
    fn out_frame_roundtrip_matches_wire_bytes() {
        let buf = hex_to_frame(TERMINATE_LAYER_FRAME_HEX);
        let f = OutFrame::decode(&buf).unwrap();
        let encoded = f.encode();
        assert_eq!(encoded.as_slice(), buf.as_slice());
    }

    #[test]
    fn out_frame_default_is_all_false_with_magic() {
        let buf = OutFrame::default().encode();
        assert_eq!(&buf[0..2], &MAGIC);
        assert_eq!(&buf[240..242], &MAGIC);
        assert_eq!(buf[10], BYTE10_CONST);
        assert_eq!(buf[16], OUT_BOOL_FALSE);
        assert_eq!(buf[22], OUT_BOOL_FALSE);
        let heartbeat = u16::from_le_bytes([buf[14], buf[15]]);
        assert_eq!(heartbeat, 0);
    }

    #[test]
    fn out_frame_rejects_missing_magic() {
        let mut buf = OutFrame::default().encode();
        buf[0] = 0;
        assert!(matches!(
            OutFrame::decode(&buf),
            Err(FrameError::BadHeader(_, _))
        ));

        let mut buf = OutFrame::default().encode();
        buf[241] = 0;
        assert!(matches!(
            OutFrame::decode(&buf),
            Err(FrameError::BadFooter(_, _))
        ));
    }

    #[test]
    fn in_frame_decodes_all_zero_idle() {
        let buf = [0u8; FRAME_LEN];
        let f = InFrame::decode(&buf).unwrap();
        assert!(!f.deposit);
        assert!(!f.acp);
        assert!(!f.auto_scaling);
    }

    #[test]
    fn in_frame_decodes_deposit_acp_active() {
        // Mirrors PCS state at t=866.65s in deposition.pcapng (frame #54189):
        // Deposit + ACP on, AutoScaling off. PCS frames have no magic.
        let mut buf = [0u8; FRAME_LEN];
        buf[118] = 0x01;
        buf[124] = 0x01;
        let f = InFrame::decode(&buf).unwrap();
        assert!(f.deposit);
        assert!(f.acp);
        assert!(!f.auto_scaling);
    }

    #[test]
    fn in_frame_decodes_autoscaling_pulse() {
        // Mirrors PCS state at t=26.50s in deposition.pcapng (frame #1084):
        // AutoScaling pulsed on alone to trigger Data Correction.
        let mut buf = [0u8; FRAME_LEN];
        buf[126] = 0x01;
        let f = InFrame::decode(&buf).unwrap();
        assert!(!f.deposit);
        assert!(!f.acp);
        assert!(f.auto_scaling);
    }

    #[test]
    fn in_frame_roundtrip() {
        let original = InFrame {
            deposit: true,
            acp: true,
            auto_scaling: false,
        };
        let bytes = original.encode();
        let decoded = InFrame::decode(&bytes).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(
            OutFrame::decode(&[0u8; 100]),
            Err(FrameError::BadLength(100))
        );
        assert_eq!(
            InFrame::decode(&[0u8; 100]),
            Err(FrameError::BadLength(100))
        );
    }
}
