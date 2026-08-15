fn main() {
    let path = std::env::args().nth(1).expect("usage: rehash <file>");
    let data = std::fs::read(&path).expect("read");
    println!(
        "{} bytes -> {}",
        data.len(),
        catalyrst_hashing::hash_bytes_v1(&data)
    );
}
