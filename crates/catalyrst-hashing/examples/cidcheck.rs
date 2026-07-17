fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: cidcheck <file> [expected]");
    let expected = a.next();
    let data = std::fs::read(&path).expect("read file");
    let got = catalyrst_hashing::hash_bytes_v1(&data);
    println!("len      = {}", data.len());
    println!("computed = {got}");
    if let Some(exp) = expected {
        println!("expected = {exp}");
        println!("MATCH    = {}", got == exp);
        println!(
            "verify_hash = {}",
            catalyrst_hashing::verify_hash(&data, &exp)
        );
    }
}
