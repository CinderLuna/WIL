use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    // Top-level / Declarations
    #[token("type")]
    Type,
    #[token("schema")]
    Schema,
    #[token("route")]
    Route,
    #[token("page")]
    Page,
    #[token("ui")]
    Ui,
    #[token("fn")]
    Fn,
    #[token("action")]
    Action,

    // Control flow & Statements
    #[token("let")]
    Let,
    #[token("return")]
    Return,
    #[token("throw")]
    Throw,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("in")]
    In,

    // Built-in Web Primitives (Contextual keywords)
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
    #[token("env")]
    Env,

    // Primitive Types
    #[token("int")]
    TypeInt,
    #[token("float")]
    TypeFloat,
    #[token("str")]
    TypeStr,
    #[token("bool")]
    TypeBool,
    #[token("any")]
    TypeAny,
    #[token("void")]
    TypeVoid,

    // Modifiers / Route Options (Contextual keywords)
    #[token("pk")]
    Pk,
    #[token("unique")]
    Unique,
    #[token("indexed")]
    Indexed,
    #[token("auto")]
    Auto,
    #[token("default")]
    Default,
    #[token("auth")]
    Auth,
    #[token("post")]
    Post,
    #[token("get")]
    Get,
    #[token("put")]
    Put,
    #[token("delete")]
    Delete,
    #[token("rate_limit")]
    RateLimit,

    // Literals
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,

    // Operators & Punctuation
    #[token("->")]
    Arrow,
    #[token("??")]
    DoubleQuestion,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("/>")]
    SlashGt,
    #[token("</")]
    LtSlash,

    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    #[token("?")]
    Question,
    #[token("@")]
    At,

    // Dynamic Literals and Identifiers
    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    StringLiteral(String),

    #[regex(r"/[a-zA-Z0-9_\-{][a-zA-Z0-9_\-\/{}]*", |lex| lex.slice().to_string(), priority = 3)]
    RoutePath(String),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    FloatLiteral(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    IntLiteral(i64),

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),
}

impl Token {
    pub fn as_ident(&self) -> Option<String> {
        match self {
            Token::Ident(s) => Some(s.clone()),
            Token::Get => Some("get".to_string()),
            Token::Post => Some("post".to_string()),
            Token::Put => Some("put".to_string()),
            Token::Delete => Some("delete".to_string()),
            Token::Auth => Some("auth".to_string()),
            Token::Res => Some("res".to_string()),
            Token::Req => Some("req".to_string()),
            Token::Db => Some("db".to_string()),
            Token::Crypto => Some("crypto".to_string()),
            Token::Jwt => Some("jwt".to_string()),
            Token::Env => Some("env".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Pk => Some("pk".to_string()),
            Token::Unique => Some("unique".to_string()),
            Token::Indexed => Some("indexed".to_string()),
            Token::Auto => Some("auto".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: std::ops::Range<usize>,
    pub leading_newline: bool,
}

pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, (String, std::ops::Range<usize>)> {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    let mut last_end = 0;

    while let Some(res) = lexer.next() {
        let span = lexer.span();
        let leading_newline = source[last_end..span.start].contains('\n');
        last_end = span.end;

        match res {
            Ok(token) => {
                tokens.push(SpannedToken {
                    token,
                    span,
                    leading_newline,
                });
            }
            Err(_) => {
                let bad = &source[span.clone()];
                return Err((format!("Unexpected character sequence: `{}`", bad), span));
            }
        }
    }

    Ok(tokens)
}
