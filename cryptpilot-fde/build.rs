fn main() {
    shadow_rs::ShadowBuilder::builder()
        .build()
        .expect("Failed to generate build information");
}
