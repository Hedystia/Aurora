//! Versioned fixed-width protocol shared by the player and game script.

#[cfg(unix)]
pub mod transport;

use bincode::{Decode, Encode};

/// Current script protocol version.
pub const VERSION: u32 = 16;
/// Fixed byte count of an encoded envelope.
pub const ENVELOPE_BYTES: usize = 60;
/// Size of the protocol, sequence and kind prefix.
pub const HEADER_BYTES: usize = 16;
/// Size of the unframed shared-memory payload.
pub const PAYLOAD_BYTES: usize = ENVELOPE_BYTES - HEADER_BYTES;
/// Reply-only flag carrying the script component's current enabled setting.
pub const BINDING_ENABLED: u32 = 1 << 31;

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
    /// Requests the next queued scene mutation or Ack.
    FlushCommands,
    /// Updates an authored node position, selected by detail.
    MoveNode,
    /// Changes the authored node's visibility, selected by detail.
    ShowNode,
    /// Selects a registered sprite frame for an authored node.
    SelectFrame,
    /// Plays a registered scene sound as a queued mutation.
    QueueSound,
    /// Updates the scene's best score.
    RecordScore,
    /// Delivers the current best score.
    RestoreScore,
    /// Run initialization before any Start callbacks.
    Load,
    /// Run the component after every Update callback for the tick.
    LateUpdate,
    /// Creates a script-owned node under optional parent X (-1 means scene root).
    QueueCreateNode,
    /// Instantiates the registered prefab at Y under optional parent X.
    QueueInstantiatePrefab,
    /// Reparents a script-owned root under optional parent X.
    QueueReparentNode,
    /// Destroys a script-owned root and invalidates its descendants' handles.
    QueueDestroyNode,
    /// Notify a loaded component that it can start receiving gameplay callbacks.
    Enable,
    /// Notify a component that it stopped receiving gameplay callbacks.
    Disable,
    /// Tear down a loaded component and clear its scheduled callbacks and listeners.
    Destroy,
    /// Advance the component's timers separately from its update callbacks.
    Schedule,
    /// Read the enabled setting of a previously disabled component.
    PollEnabled,
    /// Registers a script-created root or prefab node before its components bind.
    RegisterPrefabNode,
    /// Attaches a compiled prefab component factory to a runtime binding.
    RegisterPrefabComponent,
    /// Removes a script-created node from script lookup and event routing.
    ReleasePrefabNode,
    /// Releases a dynamic binding after its teardown callbacks.
    ReleasePrefabComponent,
    /// Updates a script-created node's parent in the script hierarchy.
    UpdatePrefabParent,
    /// Delivers one pending resource completion to the active component.
    ProcessResources,
    /// Request a compiled component factory on a scene node.
    QueueAddComponent,
    /// Request removal of one dynamically added component.
    QueueRemoveComponent,
    /// Instantiate a registered factory for a newly assigned binding.
    RegisterComponent,
    /// Dispatch a pointer press to a hit-tested node. Detail holds the reference and pointer ID.
    PointerTouch,
    /// A physical key was released, identified by detail.
    KeyUp,
    /// A pointer moved. X and Y carry normalized coordinates and detail the pointer ID.
    PointerMove,
    /// A pointer contact ended. X and Y carry normalized coordinates and detail the pointer ID.
    PointerEnd,
    /// A pointer contact was cancelled. Detail holds the pointer ID.
    PointerCancel,
    /// Node-targeted pointer movement after hit testing.
    PointerNodeMove,
    /// Node-targeted pointer release after hit testing.
    PointerNodeEnd,
    /// Node-targeted pointer cancellation after hit testing.
    PointerNodeCancel,
    /// Marks the start of a scene before its bindings load, resetting script scene state.
    SceneStart,
    /// Mouse wheel movement for all running bindings. X is horizontal and Y vertical delta.
    MouseWheel,
    /// Mouse wheel movement over a hit-tested node. Detail holds the node reference.
    PointerNodeWheel,
    /// Local position of a scene node. Detail holds the reference.
    NodeTransform,
    /// Local scale in X and Y and rotation in the high detail bits of a scene node.
    NodeScaleRotation,
    /// Advances script tweens once per simulation tick.
    Tween,
    /// Queues a local scale change for an authored node.
    ScaleNode,
    /// Queues a local rotation change for an authored node.
    RotateNode,
    /// Replaces the text content of an authored text node.
    SetText,
    /// Shows or hides the script-controlled blocking overlay. Detail is 1 to show, 0 to hide.
    SetLoading,
    /// Queues a persisted config entry. Detail packs the operation and entry index.
    ConfigSet,
    /// Delivers a config entry as `name\0value` UTF-8 chunks. Detail packs the operation.
    ConfigValue,
    /// Delivers a secret entry as `name\0value` UTF-8 chunks. Detail packs the operation.
    SecretValue,
    /// Local position of a spatial scene node. Detail holds the reference; X, Y and Z are the position.
    NodeTransform3D,
    /// Local Euler rotation of a spatial scene node. X, Y and Z carry degrees.
    NodeRotation3D,
    /// Local scale of a spatial scene node. X, Y and Z carry the scale.
    NodeScale3D,
    /// Set the bound spatial body velocity in meters per second.
    SetLinearVelocity3D,
    /// The bound spatial body began touching another body identified by detail.
    CollisionEnter3D,
    /// The bound spatial body stopped touching another body identified by detail.
    CollisionExit3D,
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
    /// Local Z position, rotation or velocity component. Zero when unused.
    pub z: f64,
    /// Kind-specific handle, key or clip index. Zero when unused.
    pub detail: u64,
    /// Zero-based component binding. Reply values also carry the enabled flag.
    pub binding: u32,
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
            z: 1.25,
            detail: u64::MAX,
            binding: 12,
        };
        let mut bytes = message.encode()?;
        assert_eq!(bytes[..4], VERSION.to_le_bytes());
        assert_eq!(bytes[4..12], u64::MAX.to_le_bytes());
        assert_eq!(bytes[12..16], 4_u32.to_le_bytes());
        assert_eq!(bytes[16..24], 42_u64.to_le_bytes());
        assert_eq!(bytes[24..32], (-3.5_f64).to_le_bytes());
        assert_eq!(bytes[32..40], 2.0_f64.to_le_bytes());
        assert_eq!(bytes[40..48], 1.25_f64.to_le_bytes());
        assert_eq!(bytes[48..56], u64::MAX.to_le_bytes());
        assert_eq!(bytes[56..], 12_u32.to_le_bytes());
        assert_eq!(Envelope::decode(&bytes)?, message);
        bytes[12] = 255;
        assert!(Envelope::decode(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn pointer_and_scene_kinds_match_generated_script_constants() {
        assert_eq!(Kind::PointerTouch as u32, 42);
        assert_eq!(Kind::KeyUp as u32, 43);
        assert_eq!(Kind::PointerMove as u32, 44);
        assert_eq!(Kind::PointerEnd as u32, 45);
        assert_eq!(Kind::PointerCancel as u32, 46);
        assert_eq!(Kind::PointerNodeMove as u32, 47);
        assert_eq!(Kind::PointerNodeEnd as u32, 48);
        assert_eq!(Kind::PointerNodeCancel as u32, 49);
        assert_eq!(Kind::SceneStart as u32, 50);
        assert_eq!(Kind::MouseWheel as u32, 51);
        assert_eq!(Kind::PointerNodeWheel as u32, 52);
        assert_eq!(Kind::NodeTransform as u32, 53);
        assert_eq!(Kind::NodeScaleRotation as u32, 54);
        assert_eq!(Kind::Tween as u32, 55);
        assert_eq!(Kind::ScaleNode as u32, 56);
        assert_eq!(Kind::RotateNode as u32, 57);
        assert_eq!(Kind::SetText as u32, 58);
        assert_eq!(Kind::SetLoading as u32, 59);
        assert_eq!(Kind::ConfigSet as u32, 60);
        assert_eq!(Kind::ConfigValue as u32, 61);
        assert_eq!(Kind::SecretValue as u32, 62);
        assert_eq!(Kind::NodeTransform3D as u32, 63);
        assert_eq!(Kind::NodeRotation3D as u32, 64);
        assert_eq!(Kind::NodeScale3D as u32, 65);
        assert_eq!(Kind::SetLinearVelocity3D as u32, 66);
        assert_eq!(Kind::CollisionEnter3D as u32, 67);
        assert_eq!(Kind::CollisionExit3D as u32, 68);
        assert_eq!(VERSION, 16);
    }
}
