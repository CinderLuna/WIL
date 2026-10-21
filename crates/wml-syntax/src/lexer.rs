use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone, Copy)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
pub enum Token {
    #[token("type")]
    Type,
    #[token("route")]
    Route,
    #[token("page")]
    Page,
    #[token("ui")]
    Ui,
    #[token("let")]
    Let,
    #[token("fn")]
    Fn,
    #[token("action")]
    Action,
    #[token("return")]
    Return,
    #[token("throw")]
    Throw,
    #[token("db")]
    Db,
    #[token("crypto")]
    Crypto,
    #[token("jwt")]
    Jwt,
    #[token("req")]
    Req,
    #[token("res")]
    Res,

    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("<")]
    LAngle,
    #[token(">")]
    RAngle,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token("->")]
    Arrow,
    #[token("??")]
    DoubleQuestion,

    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,
    #[regex(r#""([^"\\]|\\.)*""#)]
    StringLiteral,
    #[regex("[0-9]+")]
    IntLiteral,
}
