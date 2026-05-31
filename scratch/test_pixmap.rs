use std::sync::Arc;
fn main() {
    let p = tiny_skia::Pixmap::new(1, 1).unwrap();
    let a: Arc<tiny_skia::Pixmap> = Arc::new(p);
    let _b = a.clone();
}
