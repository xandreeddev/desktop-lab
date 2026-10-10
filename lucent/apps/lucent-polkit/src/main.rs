//! Composition root for the native authorization agent.
mod agent;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    agent::run()
}
