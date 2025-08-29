// Test to verify SignatureBytes has Default
use bls::SignatureBytes;

fn main() {
    // This will only compile if SignatureBytes implements Default
    let sig: SignatureBytes = Default::default();
    println!("SignatureBytes has Default implementation");
    
    // Also test that we can use it
    let _sig2 = SignatureBytes::default();
}