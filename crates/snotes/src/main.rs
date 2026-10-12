mod engine;

fn main() {
    let engine = engine::InkEngine::new();
    println!(
        "SnoteS {} (document: {:?})",
        env!("CARGO_PKG_VERSION"),
        engine.document_size()
    );
}
