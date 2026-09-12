//! crate docs
/// doc comment
fn main() {
    /* outer /* inner */ still */
    let u = "https://example.com"; // trailing
    let raw = r#"line // not comment"#;
    println!("{u}");
}
