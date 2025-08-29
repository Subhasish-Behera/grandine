#[test]
fn test_signature_bytes_has_default() {
    // This test verifies SignatureBytes implements Default
    let _sig = bls::SignatureBytes::default();
}