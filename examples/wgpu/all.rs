#[path = "../common/mod.rs"]
mod common;
#[path = "../common/layer_filter_scenes.rs"]
mod layer_filter_scenes;

mod suite;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    suite::run()
}
