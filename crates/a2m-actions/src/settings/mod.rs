//! The settings: one schema, eight layers, and for every value the layer it
//! came from (spec section 226, decision 0066).

mod load;
mod schema;

pub use load::{
    Entry, Layer, Overridden, PROJECT_FILE_NAME, PresetInfo, Settings, SourceFile, Sources,
};
pub use schema::{
    BuiltInPreset, DefaultValue, Effective, Kind, PRESETS, PROFILES, SCHEMA, Safer, Scope,
    SettingDef, find,
};
