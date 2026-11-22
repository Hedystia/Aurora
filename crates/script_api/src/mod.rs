//! Versioned fixed-width protocol shared by the player and game script.

#[cfg(target_os = "linux")]
pub mod transport;

use bincode::{Decode, Encode};

/// Current script protocol version.
pub const VERSION: u32 = 3;
/// Fixed byte count of an encoded envelope.
pub const ENVELOPE_BYTES: usize = 48;
/// Size of the protocol, sequence and kind prefix.
pub const HEADER_BYTES: usize = 16;
/// Size of the unframed shared-memory payload.
pub const PAYLOAD_BYTES: usize = ENVELOPE_BYTES - HEADER_BYTES;

/// Events sent by the player and commands returned by the script.
#[derive(Clone, Copy, Debug, Encode, Decode, PartialEq, Eq)]
pub enum Kind {
    /// Begin the script session.
    Start,
    /// Run one variable-update callback at the current simulation tick.
    Update,
    /// Run the fixed-update callback for a 60 Hz simulation tick.
    FixedUpdate,
    /// Acknowledge the frame without a mutation.
    Ack,
    /// Set the target node's local X and Y.
    SetPosition,
    /// The bound body began touching another body identified by detail.
    CollisionEnter,
    /// The bound body stopped touching another body identified by detail.
    CollisionExit,
    /// A physical key was pressed, identified by detail.
    KeyDown,
    /// A finger touched the window.
    TouchStart,
    /// Set the bound rigid body velocity in meters per second.
    SetLinearVelocity,
    /// Play a clip from the bound audio source, indexed by detail.
    PlayOneShot,
    /// Authored animation marker, identified by detail.
    AnimationEvent,
    /// Pause authored scene simulation.
    Pause,
    /// Resume authored scene simulation.
    Resume,
    /// Replace the authored scene with the registered index in detail.
    ChangeScene,
}

/// Wire message encoded with bincode 2 fixed-width, little-endian configuration.
#[derive(Clone, Copy, Debug, Encode, Decode, PartialEq)]
pub struct Envelope {
    /// Protocol version.
    pub protocol: u32,
    /// Session-local transaction sequence.
    pub seq: u64,
    /// Payload interpretation.
    pub kind: Kind,
    /// Opaque node handle.
    pub node: u64,
    /// Local X position or linear velocity X.
    pub x: f64,
    /// Local Y position or linear velocity Y.
    pub y: f64,
    /// Kind-specific handle, key or clip index. Zero when unused.
    pub detail: u64,
}

impl Envelope {
    /// Encodes the fixed-width wire record.
    pub fn encode(self) -> Result<[u8; ENVELOPE_BYTES], bincode::error::EncodeError> {
        let mut bytes = [0; ENVELOPE_BYTES];
        bincode::encode_into_slice(
            self,
            &mut bytes,
            bincode::config::standard().with_fixed_int_encoding(),
        )?;
        Ok(bytes)
    }

    /// Decodes a single wire record.
    pub fn decode(bytes: &[u8; ENVELOPE_BYTES]) -> Result<Self, bincode::error::DecodeError> {
        let (message, _) = bincode::decode_from_slice(
            bytes,
            bincode::config::standard().with_fixed_int_encoding(),
        )?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_offsets_match_script_clients() -> Result<(), Box<dyn std::error::Error>> {
        let message = Envelope {
            protocol: VERSION,
            seq: u64::MAX,
            kind: Kind::SetPosition,
            node: 42,
            x: -3.5,
            y: 2.0,
            detail: u64::MAX,
        };
        let mut bytes = message.encode()?;
        assert_eq!(bytes[..4], VERSION.to_le_bytes());
        assert_eq!(bytes[4..12], u64::MAX.to_le_bytes());
        assert_eq!(bytes[12..16], 4_u32.to_le_bytes());
        assert_eq!(bytes[16..24], 42_u64.to_le_bytes());
        assert_eq!(bytes[24..32], (-3.5_f64).to_le_bytes());
        assert_eq!(bytes[32..40], 2.0_f64.to_le_bytes());
        assert_eq!(bytes[40..], u64::MAX.to_le_bytes());
        assert_eq!(Envelope::decode(&bytes)?, message);
        bytes[12] = 255;
        assert!(Envelope::decode(&bytes).is_err());
        Ok(())
    }
}
