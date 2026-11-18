//! A PDF-syntax tokenizer and object parser modelled on iText's PRTokeniser / PdfReader
//! readPRObject(). Used for FDF files, /DA strings and ToUnicode CMaps.
use lopdf::{Dictionary, Object, Stream, StringFormat};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(String),
    /// decoded string bytes; true if written as a hex string
    Str(Vec<u8>, bool),
    /// decoded name, without the slash
    Name(Vec<u8>),
    Comment,
    StartArray,
    EndArray,
    StartDic,
    EndDic,
    Other(String),
}

impl Token {
    /// PRTokeniser.getStringValue()
    pub fn string_value(&self) -> String {
        match self {
            Token::Number(s) | Token::Other(s) => s.clone(),
            Token::Str(b, _) | Token::Name(b) => b.iter().map(|c| *c as char).collect(),
            Token::Comment => String::new(),
            Token::StartArray => "[".into(),
            Token::EndArray => "]".into(),
            Token::StartDic => "<<".into(),
            Token::EndDic => ">>".into(),
        }
    }
}

pub fn is_whitespace(ch: u8) -> bool {
    matches!(ch, 0 | 9 | 10 | 12 | 13 | 32)
}

pub fn is_delimiter(ch: u8) -> bool {
    matches!(ch, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%')
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub struct Lexer<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Lexer { data, pos: 0 }
    }

    fn read(&mut self) -> Option<u8> {
        let c = self.data.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// PRTokeniser.nextToken(): Ok(None) at end of input.
    pub fn next_token(&mut self) -> Result<Option<Token>, String> {
        let mut ch;
        loop {
            match self.read() {
                None => return Ok(None),
                Some(c) => {
                    ch = c;
                    if !is_whitespace(c) {
                        break;
                    }
                }
            }
        }
        match ch {
            b'[' => Ok(Some(Token::StartArray)),
            b']' => Ok(Some(Token::EndArray)),
            b'/' => {
                let mut name = Vec::new();
                while let Some(&c) = self.data.get(self.pos) {
                    if is_whitespace(c) || is_delimiter(c) {
                        break;
                    }
                    self.pos += 1;
                    if c == b'#' && self.pos + 1 < self.data.len() {
                        if let (Some(h), Some(l)) = (hex_val(self.data[self.pos]), hex_val(self.data[self.pos + 1])) {
                            name.push(h << 4 | l);
                            self.pos += 2;
                            continue;
                        }
                    }
                    name.push(c);
                }
                Ok(Some(Token::Name(name)))
            }
            b'>' => {
                if self.read() != Some(b'>') {
                    return Err("'>' not expected".into());
                }
                Ok(Some(Token::EndDic))
            }
            b'<' => {
                let mut v1 = self.read();
                if v1 == Some(b'<') {
                    return Ok(Some(Token::StartDic));
                }
                let mut out = Vec::new();
                loop {
                    while v1.map(is_whitespace).unwrap_or(false) {
                        v1 = self.read();
                    }
                    if v1 == Some(b'>') {
                        break;
                    }
                    let h = v1.and_then(hex_val).ok_or("Error reading string")?;
                    let mut v2 = self.read();
                    while v2.map(is_whitespace).unwrap_or(false) {
                        v2 = self.read();
                    }
                    if v2 == Some(b'>') {
                        out.push(h << 4);
                        break;
                    }
                    let l = v2.and_then(hex_val).ok_or("Error reading string")?;
                    out.push(h << 4 | l);
                    v1 = self.read();
                }
                Ok(Some(Token::Str(out, true)))
            }
            b'%' => {
                while let Some(c) = self.read() {
                    if c == b'\r' || c == b'\n' {
                        break;
                    }
                }
                Ok(Some(Token::Comment))
            }
            b'(' => {
                let mut out = Vec::new();
                let mut nesting = 0;
                loop {
                    let mut c = self.read().ok_or("Error reading string")?;
                    if c == b'(' {
                        nesting += 1;
                    } else if c == b')' {
                        nesting -= 1;
                    } else if c == b'\\' {
                        let mut line_break = false;
                        c = self.read().ok_or("Error reading string")?;
                        match c {
                            b'n' => c = b'\n',
                            b'r' => c = b'\r',
                            b't' => c = b'\t',
                            b'b' => c = 0x08,
                            b'f' => c = 0x0c,
                            b'(' | b')' | b'\\' => {}
                            b'\r' => {
                                line_break = true;
                                if self.data.get(self.pos) == Some(&b'\n') {
                                    self.pos += 1;
                                }
                            }
                            b'\n' => line_break = true,
                            b'0'..=b'7' => {
                                let mut octal = (c - b'0') as u32;
                                for _ in 0..2 {
                                    match self.data.get(self.pos) {
                                        Some(&d) if (b'0'..=b'7').contains(&d) => {
                                            octal = (octal << 3) + (d - b'0') as u32;
                                            self.pos += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                c = (octal & 0xff) as u8;
                            }
                            _ => {}
                        }
                        if line_break {
                            continue;
                        }
                        out.push(c);
                        continue;
                    } else if c == b'\r' {
                        if self.data.get(self.pos) == Some(&b'\n') {
                            self.pos += 1;
                        }
                        c = b'\n';
                    }
                    if nesting == -1 {
                        break;
                    }
                    out.push(c);
                }
                Ok(Some(Token::Str(out, false)))
            }
            _ => {
                let mut s = String::new();
                if ch == b'-' || ch == b'+' || ch == b'.' || ch.is_ascii_digit() {
                    s.push(ch as char);
                    while let Some(&c) = self.data.get(self.pos) {
                        if c.is_ascii_digit() || c == b'.' {
                            s.push(c as char);
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    Ok(Some(Token::Number(s)))
                } else {
                    s.push(ch as char);
                    while let Some(&c) = self.data.get(self.pos) {
                        if is_whitespace(c) || is_delimiter(c) {
                            break;
                        }
                        s.push(c as char);
                        self.pos += 1;
                    }
                    Ok(Some(Token::Other(s)))
                }
            }
        }
    }

    /// PRTokeniser.nextValidToken(): skips comments. (Reference detection is done by the caller.)
    pub fn next_valid_token(&mut self) -> Result<Option<Token>, String> {
        loop {
            match self.next_token()? {
                Some(Token::Comment) => continue,
                t => return Ok(t),
            }
        }
    }

    fn number(s: &str) -> Result<Object, String> {
        if s.contains('.') {
            s.parse::<f32>().map(Object::Real).map_err(|_| format!("bad number {}", s))
        } else {
            match s.parse::<i64>() {
                Ok(i) => Ok(Object::Integer(i)),
                Err(_) => s.parse::<f32>().map(Object::Real).map_err(|_| format!("bad number {}", s)),
            }
        }
    }

    /// PdfReader.readPRObject()
    pub fn read_object(&mut self) -> Result<Object, String> {
        let tok = self.next_valid_token()?.ok_or("unexpected end of file")?;
        self.object_from(tok, 0)
    }

    fn object_from(&mut self, tok: Token, depth: usize) -> Result<Object, String> {
        if depth > 500 {
            return Err("nesting too deep".into());
        }
        match tok {
            Token::StartDic => {
                let mut dic = Dictionary::new();
                loop {
                    let t = self.next_valid_token()?.ok_or("unexpected end of file")?;
                    let key = match t {
                        Token::EndDic => break,
                        Token::Name(n) => n,
                        _ => return Err("Dictionary key is not a name.".into()),
                    };
                    let vt = self.next_valid_token()?.ok_or("unexpected end of file")?;
                    if matches!(vt, Token::EndDic | Token::EndArray) {
                        return Err("Unexpected '>>' or ']'".into());
                    }
                    let v = self.object_from(vt, depth + 1)?;
                    dic.set(key, v);
                }
                // stream?
                let save = self.pos;
                if let Ok(Some(Token::Other(s))) = self.next_token() {
                    if s == "stream" {
                        let mut ch = self.read();
                        while matches!(ch, Some(32) | Some(9) | Some(0) | Some(12)) {
                            ch = self.read();
                        }
                        if ch != Some(b'\n') {
                            ch = self.read();
                        }
                        if ch != Some(b'\n') && ch.is_some() {
                            self.pos -= 1;
                        }
                        let start = self.pos;
                        let declared = match dic.get(b"Length") {
                            Ok(Object::Integer(n)) if *n >= 0 => Some(*n as usize),
                            _ => None,
                        };
                        let ok_len = declared.filter(|n| {
                            let e = start + n;
                            e <= self.data.len() && {
                                let rest = &self.data[e..];
                                let skip = rest.iter().take_while(|c| is_whitespace(**c)).count();
                                rest[skip..].starts_with(b"endstream")
                            }
                        });
                        let end = match ok_len {
                            Some(n) => start + n,
                            None => {
                                let rel = self.data[start..]
                                    .windows(9)
                                    .position(|w| w == b"endstream")
                                    .ok_or("endstream not found")?;
                                let mut e = start + rel;
                                if e > start && self.data[e - 1] == b'\n' {
                                    e -= 1;
                                }
                                if e > start && self.data[e - 1] == b'\r' {
                                    e -= 1;
                                }
                                e
                            }
                        };
                        let content = self.data[start..end].to_vec();
                        self.pos = end;
                        return Ok(Object::Stream(Stream::new(dic, content)));
                    }
                }
                self.pos = save;
                Ok(Object::Dictionary(dic))
            }
            Token::StartArray => {
                let mut arr = Vec::new();
                loop {
                    let t = self.next_valid_token()?.ok_or("unexpected end of file")?;
                    match t {
                        Token::EndArray => break,
                        Token::EndDic => return Err("Unexpected '>>'".into()),
                        t => arr.push(self.object_from(t, depth + 1)?),
                    }
                }
                Ok(Object::Array(arr))
            }
            Token::Number(s) => {
                // "num gen R"?
                let save = self.pos;
                if let Ok(Some(Token::Number(g))) = self.next_token() {
                    if let Ok(Some(Token::Other(r))) = self.next_token() {
                        if r == "R" {
                            if let (Ok(n), Ok(g)) = (s.parse::<u32>(), g.parse::<u16>()) {
                                return Ok(Object::Reference((n, g)));
                            }
                        }
                    }
                }
                self.pos = save;
                Self::number(&s)
            }
            Token::Str(b, hex) => Ok(Object::String(b, if hex { StringFormat::Hexadecimal } else { StringFormat::Literal })),
            Token::Name(n) => Ok(Object::Name(n)),
            Token::Other(s) => match s.as_str() {
                "true" => Ok(Object::Boolean(true)),
                "false" => Ok(Object::Boolean(false)),
                _ => Ok(Object::Null), // "null", and iText's PdfLiteral for anything else
            },
            Token::EndArray | Token::EndDic => Err("unexpected closing token".into()),
            Token::Comment => Err("comment".into()),
        }
    }
}
