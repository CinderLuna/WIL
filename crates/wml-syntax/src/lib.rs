pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{AstArena, Program};
pub use lexer::{tokenize, SpannedToken, Token};
pub use parser::{ParseError, Parser};

pub fn parse(source: &str) -> Result<(Program, AstArena), ParseError> {
    let tokens = tokenize(source).map_err(|(message, span)| ParseError { message, span })?;
    let mut arena = AstArena::new();
    let program = {
        let mut parser = Parser::new(source, &tokens, &mut arena);
        parser.parse_program()?
    };
    Ok((program, arena))
}
