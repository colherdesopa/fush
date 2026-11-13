mod ast;
mod lexer;
mod parser;

fn main() {
    let src = std::env::args().nth(1).unwrap_or_default();
    match parser::parse(&src) {
        Ok(program) => println!("{program:#?}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
