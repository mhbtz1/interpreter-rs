use std::io::Write;

fn main() {
    let alignment = size_of::<usize>() * 2;
    let usize_alignment = std::mem::align_of::<usize>();
    println!("Alignment: {}", alignment);
    println!("Usize alignment: {}", usize_alignment);

    loop {
        print!(">");
        std::io::stdout().flush().expect("Did not flush properly!");
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).unwrap();
        print!("Just read {}", s);
    }
}
