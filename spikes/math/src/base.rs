fn main(){ let s: Vec<String> = serde_json::from_str(&std::fs::read_to_string("samples.json").unwrap()).unwrap(); println!("{}", s.len()); }
