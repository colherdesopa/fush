mod ast;
mod builtins;
mod exec;
mod lexer;
mod parser;
mod runtime;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut ast_mode = false;
    let mut src: Option<String> = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--ast" => ast_mode = true,
            "-c" => match iter.next().cloned() {
                Some(s) => src = Some(s),
                None => {
                    eprintln!("fush: -c requires a command");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("usage: fush [--ast] [-c 'command'] [script]");
                return;
            }
            _ => {
                match std::fs::read_to_string(arg) {
                    Ok(text) => src = Some(text),
                    Err(e) => {
                        eprintln!("fush: cannot read '{arg}': {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    let src = src.unwrap_or_default();
    if ast_mode {
        run_ast_mode(&src);
        return;
    }
    let program = match parser::parse(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut rt = runtime::Runtime::new();
    let status = exec::run_program(&mut rt, &program.stmts);
    std::process::exit(status);
}

fn run_ast_mode(src: &str) {
    match parser::parse(src) {
        Ok(program) => println!("{program:#?}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
