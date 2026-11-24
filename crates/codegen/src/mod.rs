// Codegen module
pub mod metadata;

pub use metadata::{
    Metadata, NodeFilter, Property, PropertyType, SDK_DECLARATIONS, SDK_MODULES, extract, lower,
    lower_runtime, protocol_typescript,
};
