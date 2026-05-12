// Phase 2: register custom derivative functions here.
pub struct PluginRegistry;

impl PluginRegistry {
    pub fn new() -> Self { PluginRegistry }
}

impl Default for PluginRegistry {
    fn default() -> Self { Self::new() }
}
