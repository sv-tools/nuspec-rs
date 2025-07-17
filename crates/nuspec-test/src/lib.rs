#[unsafe(no_mangle)]
pub extern "C" fn hello() {
    println!("Hello from Rust!");
}

pub fn main() {
    println!("This is the main function in the nuspec-test crate.");
}
