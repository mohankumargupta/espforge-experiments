use espf::context::{ResolvedDevice, RuntimeContext};

pub fn tmp102(device: &ResolvedDevice, ctx: &RuntimeContext) -> Result<(), String> {
    crate::drivers::generated_registry::route_driver("tmp102", device, ctx)
}
