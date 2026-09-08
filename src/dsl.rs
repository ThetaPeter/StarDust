//! Layout DSL: a thin sugar copy of dumpable IR. Placement always comes from
//! [`StructBuilder::repr_c`] or [`ColumnarBuilder::build`]. `Ir::to_sd` prints
//! only when every layout equals that rebuild.

use std::path::Path;

use crate::builder::{BitpackBuilder, ColumnarBuilder, StructBuilder};
use crate::error::{Error, Result};
use crate::ir::{
    BitOrder, BitpackType, Class, ColumnarType, Endianness, Import, Ir, Lang, Param, Ret, Scalar,
    StructType, Type,
};

impl Ir {
    pub fn from_sd(src: &str) -> Result<Self> {
        parse(src)
    }

    pub fn from_sd_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path)
            .map_err(|e| Error::Dsl(format!("failed to read {}: {e}", path.display())))?;
        Self::from_sd(&src)
    }

    pub fn to_sd(&self) -> Result<String> {
        for (name, ty) in &self.layouts {
            match ty {
                Type::Struct(_) => ensure_repr_c(name, ty)?,
                Type::Columnar(_) => ensure_columnar(name, ty)?,
                Type::Bitpack(_) => ensure_bitpack(name, ty)?,
                _ => {
                    return Err(Error::Dsl(format!(
                        "layout {name} cannot be printed as DSL sugar"
                    )));
                }
            }
        }
        Ok(print_sd(self))
    }
}

fn dsl_err(line: usize, col: usize, msg: impl Into<String>) -> Error {
    Error::Dsl(format!("{}:{}: {}", line, col, msg.into()))
}

// --- reconstruct / print-time check (ignore stored offsets; run repr_c) ---

fn strip_offsets(ty: &Type) -> Result<Type> {
    match ty {
        Type::Scalar(s) => Ok(Type::scalar(*s)),
        Type::Slice(s) => Ok(Type::slice(strip_offsets(&s.elem)?)),
        Type::Array(a) => Type::array(strip_offsets(&a.elem)?, a.count),
        Type::Struct(s) => rebuild_repr_c(s),
        Type::Columnar(c) => rebuild_columnar(c),
        Type::Bitpack(b) => rebuild_bitpack(b),
        Type::Link => Ok(Type::link()),
    }
}

fn rebuild_repr_c(s: &StructType) -> Result<Type> {
    let mut b = StructBuilder::new();
    for f in &s.fields {
        b = b.field(f.name.clone(), strip_offsets(&f.ty)?);
    }
    b.repr_c()
}

fn rebuild_columnar(c: &ColumnarType) -> Result<Type> {
    let mut b = ColumnarBuilder::new(c.capacity);
    for f in &c.columns {
        b = b.column(f.name.clone(), strip_offsets(&f.ty)?)?;
    }
    b.build()
}

fn rebuild_bitpack(b: &BitpackType) -> Result<Type> {
    let mut builder = match b.container {
        Some(container) => BitpackBuilder::new(container),
        None => BitpackBuilder::span(b.span)?,
    };
    for f in &b.fields {
        builder = builder.field(f.name.clone(), f.width);
    }
    builder.build()
}

fn ensure_repr_c(layout_name: &str, ty: &Type) -> Result<()> {
    match ty {
        Type::Scalar(_) | Type::Link => Ok(()),
        Type::Slice(s) => ensure_repr_c(layout_name, &s.elem),
        Type::Array(a) => ensure_repr_c(layout_name, &a.elem),
        Type::Struct(s) => {
            for f in &s.fields {
                ensure_repr_c(layout_name, &f.ty)?;
            }
            let rebuilt = rebuild_repr_c(s)?;
            if &rebuilt != ty {
                return Err(Error::Dsl(format!(
                    "layout {layout_name} is not equal to repr_c of its fields"
                )));
            }
            Ok(())
        }
        Type::Columnar(_) => ensure_columnar(layout_name, ty),
        Type::Bitpack(_) => Err(Error::Dsl(format!(
            "layout {layout_name} cannot nest bitpack in repr_c"
        ))),
    }
}

fn ensure_columnar(layout_name: &str, ty: &Type) -> Result<()> {
    let Type::Columnar(c) = ty else {
        return Err(Error::Dsl(format!(
            "layout {layout_name} is not a columnar layout"
        )));
    };
    for col in &c.columns {
        ensure_repr_c(layout_name, &col.ty)?;
    }
    let rebuilt = rebuild_columnar(c)?;
    if &rebuilt != ty {
        return Err(Error::Dsl(format!(
            "layout {layout_name} is not equal to ColumnarBuilder of its columns"
        )));
    }
    Ok(())
}

fn ensure_bitpack(layout_name: &str, ty: &Type) -> Result<()> {
    let Type::Bitpack(b) = ty else {
        return Err(Error::Dsl(format!(
            "layout {layout_name} is not a bitpack layout"
        )));
    };
    let rebuilt = rebuild_bitpack(b)?;
    if &rebuilt != ty {
        return Err(Error::Dsl(format!(
            "layout {layout_name} is not equal to BitpackBuilder of its fields"
        )));
    }
    Ok(())
}

// --- tokens ---

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokKind {
    Ident(String),
    Number(u64),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Semicolon,
    Comma,
    Arrow,
    Eof,
}

#[derive(Debug, Clone)]
struct Tok {
    kind: TokKind,
    line: usize,
    col: usize,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn bump_byte(&mut self) -> Option<u8> {
        let b = self.peek_byte()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek_byte() {
                Some(b) if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' => {
                    self.bump_byte();
                }
                Some(b'/') => {
                    let next = self.src.as_bytes().get(self.pos + 1).copied();
                    if next == Some(b'/') {
                        self.bump_byte();
                        self.bump_byte();
                        while let Some(b) = self.peek_byte() {
                            if b == b'\n' {
                                break;
                            }
                            self.bump_byte();
                        }
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn next_tok(&mut self) -> Result<Tok> {
        self.skip_ws_and_comments();
        let line = self.line;
        let col = self.col;
        let Some(b) = self.peek_byte() else {
            return Ok(Tok {
                kind: TokKind::Eof,
                line,
                col,
            });
        };
        let kind = match b {
            b'{' => {
                self.bump_byte();
                TokKind::LBrace
            }
            b'}' => {
                self.bump_byte();
                TokKind::RBrace
            }
            b'(' => {
                self.bump_byte();
                TokKind::LParen
            }
            b')' => {
                self.bump_byte();
                TokKind::RParen
            }
            b'[' => {
                self.bump_byte();
                TokKind::LBracket
            }
            b']' => {
                self.bump_byte();
                TokKind::RBracket
            }
            b':' => {
                self.bump_byte();
                TokKind::Colon
            }
            b';' => {
                self.bump_byte();
                TokKind::Semicolon
            }
            b',' => {
                self.bump_byte();
                TokKind::Comma
            }
            b'-' => {
                self.bump_byte();
                if self.peek_byte() != Some(b'>') {
                    return Err(dsl_err(line, col, "expected '->'"));
                }
                self.bump_byte();
                TokKind::Arrow
            }
            b'0'..=b'9' => {
                let start = self.pos;
                while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    self.bump_byte();
                }
                let text = &self.src[start..self.pos];
                let n = text
                    .parse::<u64>()
                    .map_err(|_| dsl_err(line, col, format!("invalid number {text}")))?;
                TokKind::Number(n)
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = self.pos;
                self.bump_byte();
                while matches!(
                    self.peek_byte(),
                    Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
                ) {
                    self.bump_byte();
                }
                TokKind::Ident(self.src[start..self.pos].to_string())
            }
            _ => {
                return Err(dsl_err(
                    line,
                    col,
                    format!("unexpected character {:?}", b as char),
                ));
            }
        };
        Ok(Tok { kind, line, col })
    }

    fn tokenize(mut self) -> Result<Vec<Tok>> {
        let mut out = Vec::new();
        loop {
            let tok = self.next_tok()?;
            let is_eof = matches!(tok.kind, TokKind::Eof);
            out.push(tok);
            if is_eof {
                return Ok(out);
            }
        }
    }
}

// --- parser ---

struct Parser {
    toks: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self {
        Self { toks, i: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.i]
    }

    fn peek_kind(&self) -> &TokKind {
        &self.peek().kind
    }

    fn error_here(&self, msg: impl Into<String>) -> Error {
        let t = self.peek();
        dsl_err(t.line, t.col, msg)
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.i].clone();
        if !matches!(t.kind, TokKind::Eof) {
            self.i += 1;
        }
        t
    }

    fn expect_ident(&mut self) -> Result<String> {
        let t = self.peek().clone();
        match t.kind {
            TokKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => Err(dsl_err(
                t.line,
                t.col,
                format!("expected identifier, got {other:?}"),
            )),
        }
    }

    fn expect_kw(&mut self, kw: &str) -> Result<()> {
        match self.peek_kind() {
            TokKind::Ident(s) if s == kw => {
                self.bump();
                Ok(())
            }
            _ => Err(self.error_here(format!("expected `{kw}`"))),
        }
    }

    fn expect(&mut self, want: TokKind, label: &str) -> Result<()> {
        if self.peek_kind() == &want {
            self.bump();
            Ok(())
        } else {
            Err(self.error_here(format!("expected {label}")))
        }
    }

    fn parse_file(&mut self) -> Result<Ir> {
        self.expect_kw("endian")?;
        let endian_tok = self.peek().clone();
        let endianness = {
            let id = self.expect_ident()?;
            match id.as_str() {
                "little" => Endianness::Little,
                "big" => Endianness::Big,
                other => {
                    return Err(dsl_err(
                        endian_tok.line,
                        endian_tok.col,
                        format!("endian must be little or big, got `{other}`"),
                    ));
                }
            }
        };
        let mut ir = Ir::new(endianness);
        if matches!(self.peek_kind(), TokKind::Ident(s) if s == "bit_order") {
            self.bump();
            let order_tok = self.peek().clone();
            let id = self.expect_ident()?;
            match id.as_str() {
                "lsb0" => {}
                "msb0" => return Err(Error::UnsupportedBitOrder),
                other => {
                    return Err(dsl_err(
                        order_tok.line,
                        order_tok.col,
                        format!("bit_order must be lsb0 or msb0, got `{other}`"),
                    ));
                }
            }
        }
        loop {
            match self.peek_kind().clone() {
                TokKind::Eof => break,
                TokKind::Ident(s) if s == "layout" => self.parse_layout(&mut ir)?,
                TokKind::Ident(s) if s == "import" => self.parse_import(&mut ir)?,
                TokKind::Ident(s) if s == "class" => self.parse_class(&mut ir)?,
                _ => {
                    return Err(self.error_here("expected `layout`, `import`, or `class`"));
                }
            }
        }
        ir.validate_class_layouts()?;
        Ok(ir)
    }

    fn parse_layout(&mut self, ir: &mut Ir) -> Result<()> {
        self.expect_kw("layout")?;
        let name = self.expect_ident()?;
        let ty = match self.peek_kind().clone() {
            TokKind::Ident(s) if s == "repr_c" => {
                self.bump();
                self.parse_struct_body()?
            }
            TokKind::Ident(s) if s == "columnar" => {
                self.bump();
                self.parse_columnar_body()?
            }
            TokKind::Ident(s) if s == "bitpack" => {
                self.bump();
                self.parse_bitpack_body()?
            }
            _ => return Err(self.error_here("expected `repr_c`, `columnar`, or `bitpack`")),
        };
        if ir.layouts.contains_key(&name) {
            return Err(self.error_here(format!("duplicate layout `{name}`")));
        }
        ir.insert(name, ty);
        Ok(())
    }

    fn parse_columnar_body(&mut self) -> Result<Type> {
        self.expect(TokKind::LParen, "`(`")?;
        let cap_tok = self.peek().clone();
        let capacity = match cap_tok.kind {
            TokKind::Number(n) => {
                self.bump();
                n
            }
            _ => {
                return Err(dsl_err(
                    cap_tok.line,
                    cap_tok.col,
                    "expected columnar capacity",
                ));
            }
        };
        self.expect(TokKind::RParen, "`)`")?;
        self.expect(TokKind::LBrace, "`{`")?;
        let mut b = ColumnarBuilder::new(capacity);
        while !matches!(self.peek_kind(), TokKind::RBrace) {
            if matches!(self.peek_kind(), TokKind::Eof) {
                return Err(self.error_here("unclosed `{`; expected column or `}`"));
            }
            let field_tok = self.peek().clone();
            let field_name = match field_tok.kind {
                TokKind::Ident(s) => {
                    self.bump();
                    s
                }
                other => {
                    return Err(dsl_err(
                        field_tok.line,
                        field_tok.col,
                        format!("expected column name, got {other:?}"),
                    ));
                }
            };
            self.expect(TokKind::Colon, "`:`")?;
            let ty = self.parse_type()?;
            self.expect(TokKind::Semicolon, "`;`")?;
            b = b.column(field_name, ty)?;
        }
        self.expect(TokKind::RBrace, "`}`")?;
        b.build()
    }

    fn parse_bitpack_body(&mut self) -> Result<Type> {
        let tok = self.peek().clone();
        let builder = match &tok.kind {
            TokKind::Ident(s) if s == "span" => {
                self.bump();
                if matches!(
                    self.peek_kind(),
                    TokKind::Ident(id) if unsigned_bitpack_container(id).is_some()
                ) {
                    return Err(Error::InvalidLayout);
                }
                let n_tok = self.peek().clone();
                let n = match n_tok.kind {
                    TokKind::Number(n) => {
                        self.bump();
                        n
                    }
                    _ => {
                        return Err(dsl_err(n_tok.line, n_tok.col, "expected bitpack span"));
                    }
                };
                if matches!(
                    self.peek_kind(),
                    TokKind::Ident(id) if unsigned_bitpack_container(id).is_some()
                ) {
                    return Err(Error::InvalidLayout);
                }
                BitpackBuilder::span(n)?
            }
            TokKind::Ident(s) => {
                let Some(container) = unsigned_bitpack_container(s) else {
                    return Err(dsl_err(
                        tok.line,
                        tok.col,
                        format!("bitpack container must be u8, u16, u32, or u64, got `{s}`"),
                    ));
                };
                self.bump();
                if matches!(self.peek_kind(), TokKind::Ident(id) if id == "span") {
                    return Err(Error::InvalidLayout);
                }
                BitpackBuilder::new(container)
            }
            _ => {
                return Err(dsl_err(
                    tok.line,
                    tok.col,
                    "expected bitpack container type or `span`",
                ));
            }
        };
        self.parse_bitpack_fields(builder)
    }

    fn parse_bitpack_fields(&mut self, mut b: BitpackBuilder) -> Result<Type> {
        self.expect(TokKind::LBrace, "`{`")?;
        while !matches!(self.peek_kind(), TokKind::RBrace) {
            if matches!(self.peek_kind(), TokKind::Eof) {
                return Err(self.error_here("unclosed `{`; expected field or `}`"));
            }
            let field_tok = self.peek().clone();
            let field_name = match field_tok.kind {
                TokKind::Ident(s) => {
                    self.bump();
                    s
                }
                other => {
                    return Err(dsl_err(
                        field_tok.line,
                        field_tok.col,
                        format!("expected field name, got {other:?}"),
                    ));
                }
            };
            self.expect(TokKind::Colon, "`:`")?;
            let width_tok = self.peek().clone();
            let width = match width_tok.kind {
                TokKind::Number(n) => {
                    self.bump();
                    n
                }
                _ => {
                    return Err(dsl_err(width_tok.line, width_tok.col, "expected bit width"));
                }
            };
            self.expect(TokKind::Semicolon, "`;`")?;
            b = b.field(field_name, width);
        }
        self.expect(TokKind::RBrace, "`}`")?;
        b.build()
    }

    fn parse_struct_body(&mut self) -> Result<Type> {
        self.expect(TokKind::LBrace, "`{`")?;
        let mut b = StructBuilder::new();
        while !matches!(self.peek_kind(), TokKind::RBrace) {
            if matches!(self.peek_kind(), TokKind::Eof) {
                return Err(self.error_here("unclosed `{`; expected field or `}`"));
            }
            let field_tok = self.peek().clone();
            let field_name = match field_tok.kind {
                TokKind::Ident(s) => {
                    self.bump();
                    s
                }
                other => {
                    return Err(dsl_err(
                        field_tok.line,
                        field_tok.col,
                        format!("expected field name, got {other:?}"),
                    ));
                }
            };
            self.expect(TokKind::Colon, "`:`")?;
            let ty = self.parse_type()?;
            self.expect(TokKind::Semicolon, "`;`")?;
            b = b.field(field_name, ty);
        }
        self.expect(TokKind::RBrace, "`}`")?;
        b.repr_c()
    }

    fn parse_type(&mut self) -> Result<Type> {
        let tok = self.peek().clone();
        match tok.kind {
            TokKind::LBracket => {
                self.bump();
                let elem = self.parse_type()?;
                self.expect(TokKind::Semicolon, "`;` in array type")?;
                let count_tok = self.peek().clone();
                let count = match count_tok.kind {
                    TokKind::Number(n) => {
                        self.bump();
                        n
                    }
                    _ => {
                        return Err(dsl_err(
                            count_tok.line,
                            count_tok.col,
                            "expected array count",
                        ))
                    }
                };
                self.expect(TokKind::RBracket, "`]`")?;
                Type::array(elem, count)
            }
            TokKind::Ident(s) if s == "slice" => {
                self.bump();
                Ok(Type::slice(self.parse_type()?))
            }
            TokKind::Ident(s) if s == "struct" => {
                self.bump();
                self.expect_kw("repr_c")?;
                self.parse_struct_body()
            }
            TokKind::Ident(s) if s == "link" => {
                self.bump();
                Ok(Type::link())
            }
            TokKind::Ident(s) => {
                if let Some(sc) = scalar_from_name(&s) {
                    self.bump();
                    Ok(Type::scalar(sc))
                } else {
                    Err(dsl_err(
                        tok.line,
                        tok.col,
                        format!(
                            "unknown type `{s}` (no named type refs; use inline `struct repr_c {{ ... }}`)"
                        ),
                    ))
                }
            }
            _ => Err(dsl_err(tok.line, tok.col, "expected type")),
        }
    }

    fn parse_import(&mut self, ir: &mut Ir) -> Result<()> {
        self.expect_kw("import")?;
        let name = self.expect_ident()?;
        let import = self.parse_sig_body(false)?;
        ir.insert_import(name, import)
    }

    fn parse_class(&mut self, ir: &mut Ir) -> Result<()> {
        self.expect_kw("class")?;
        let name = self.expect_ident()?;
        if ir.classes.contains_key(&name) {
            return Err(Error::DuplicateClass);
        }
        self.expect(TokKind::LBrace, "`{`")?;
        let mut layout: Option<String> = None;
        let mut class = Class {
            layout: String::new(),
            base: None,
            methods: Default::default(),
        };
        while !matches!(self.peek_kind(), TokKind::RBrace) {
            if matches!(self.peek_kind(), TokKind::Eof) {
                return Err(
                    self.error_here("unclosed `{`; expected `layout`, `base`, `method`, or `}`")
                );
            }
            match self.peek_kind().clone() {
                TokKind::Ident(s) if s == "layout" => {
                    self.bump();
                    if layout.is_some() {
                        return Err(self.error_here("duplicate `layout` in class"));
                    }
                    layout = Some(self.expect_ident()?);
                    self.expect(TokKind::Semicolon, "`;`")?;
                }
                TokKind::Ident(s) if s == "base" => {
                    self.bump();
                    if layout.is_none() {
                        return Err(self.error_here("`base` must follow `layout`"));
                    }
                    if class.base.is_some() {
                        return Err(self.error_here("duplicate `base` in class"));
                    }
                    class.base = Some(self.expect_ident()?);
                    self.expect(TokKind::Semicolon, "`;`")?;
                }
                TokKind::Ident(s) if s == "method" => {
                    self.bump();
                    let method_name = self.expect_ident()?;
                    let import = self.parse_sig_body(true)?;
                    class.insert_method(method_name, import)?;
                }
                _ => return Err(self.error_here("expected `layout`, `base`, or `method`")),
            }
        }
        let Some(layout_name) = layout else {
            return Err(self.error_here("class requires a `layout` clause"));
        };
        self.expect(TokKind::RBrace, "`}`")?;
        class.layout = layout_name;
        ir.classes.insert(name, class);
        Ok(())
    }

    fn parse_sig_body(&mut self, require_lang: bool) -> Result<Import> {
        self.expect(TokKind::LParen, "`(`")?;
        let mut args = Vec::new();
        if !matches!(self.peek_kind(), TokKind::RParen) {
            loop {
                args.push(self.parse_param()?);
                if matches!(self.peek_kind(), TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect(TokKind::RParen, "`)`")?;
        self.expect(TokKind::Arrow, "`->`")?;
        let ret = self.parse_ret()?;
        self.expect(TokKind::LBrace, "`{`")?;
        self.expect_kw("symbol")?;
        let symbol = self.expect_ident()?;
        self.expect(TokKind::Semicolon, "`;`")?;
        let lang = if require_lang {
            self.parse_lang_clause()?
        } else if matches!(self.peek_kind(), TokKind::Ident(s) if s == "lang") {
            self.parse_lang_clause()?
        } else {
            Lang::C
        };
        self.expect(TokKind::RBrace, "`}`")?;
        Ok(Import::new(symbol, args, ret).with_lang(lang))
    }

    fn parse_lang_clause(&mut self) -> Result<Lang> {
        self.expect_kw("lang")?;
        let tok = self.peek().clone();
        let id = self.expect_ident()?;
        let lang = lang_from_ident(&id)
            .ok_or_else(|| dsl_err(tok.line, tok.col, format!("unknown lang `{id}`")))?;
        self.expect(TokKind::Semicolon, "`;`")?;
        Ok(lang)
    }

    fn parse_param(&mut self) -> Result<Param> {
        let id = self.expect_ident()?;
        match id.as_str() {
            "ptr" => Ok(Param::Ptr),
            "len" => Ok(Param::Len),
            "owned_ptr" => Ok(Param::OwnedPtr),
            other => match scalar_from_name(other) {
                Some(s) => Ok(Param::Scalar(s)),
                None => Err(self.error_here(format!(
                    "expected ptr, len, owned_ptr, or a scalar type, got `{other}`"
                ))),
            },
        }
    }

    fn parse_ret(&mut self) -> Result<Ret> {
        let id = self.expect_ident()?;
        match id.as_str() {
            "void" => Ok(Ret::Void),
            "owned_ptr" => Ok(Ret::OwnedPtr),
            other => match scalar_from_name(other) {
                Some(s) => Ok(Ret::Scalar(s)),
                None => Err(self.error_here(format!(
                    "expected void, owned_ptr, or a scalar type, got `{other}`"
                ))),
            },
        }
    }
}

fn lang_from_ident(name: &str) -> Option<Lang> {
    Some(match name {
        "c" => Lang::C,
        "python" => Lang::Python,
        "llvm" => Lang::Llvm,
        "haskell" => Lang::Haskell,
        "lean" => Lang::Lean,
        _ => return None,
    })
}

fn lang_ident(lang: Lang) -> &'static str {
    match lang {
        Lang::C => "c",
        Lang::Python => "python",
        Lang::Llvm => "llvm",
        Lang::Haskell => "haskell",
        Lang::Lean => "lean",
    }
}

fn unsigned_bitpack_container(name: &str) -> Option<Scalar> {
    match name {
        "u8" => Some(Scalar::U8),
        "u16" => Some(Scalar::U16),
        "u32" => Some(Scalar::U32),
        "u64" => Some(Scalar::U64),
        _ => None,
    }
}

fn scalar_from_name(name: &str) -> Option<Scalar> {
    Some(match name {
        "i8" => Scalar::I8,
        "u8" => Scalar::U8,
        "i16" => Scalar::I16,
        "u16" => Scalar::U16,
        "i32" => Scalar::I32,
        "u32" => Scalar::U32,
        "i64" => Scalar::I64,
        "u64" => Scalar::U64,
        "f32" => Scalar::F32,
        "f64" => Scalar::F64,
        _ => return None,
    })
}

fn parse(src: &str) -> Result<Ir> {
    let toks = Lexer::new(src).tokenize()?;
    Parser::new(toks).parse_file()
}

// --- printer (BTreeMap order; sugar only) ---

fn print_sd(ir: &Ir) -> String {
    let mut out = String::new();
    out.push_str("endian ");
    out.push_str(match ir.endianness {
        Endianness::Little => "little",
        Endianness::Big => "big",
    });
    out.push('\n');
    out.push_str("bit_order ");
    out.push_str(match ir.bit_order {
        BitOrder::Lsb0 => "lsb0",
        BitOrder::Msb0 => "msb0",
    });
    out.push('\n');

    for (name, ty) in &ir.layouts {
        out.push('\n');
        out.push_str("layout ");
        out.push_str(name);
        match ty {
            Type::Struct(s) => {
                out.push_str(" repr_c {\n");
                emit_fields(&mut out, &s.fields, 2);
                out.push_str("}\n");
            }
            Type::Columnar(c) => {
                out.push_str(" columnar(");
                out.push_str(&c.capacity.to_string());
                out.push_str(") {\n");
                emit_fields(&mut out, &c.columns, 2);
                out.push_str("}\n");
            }
            Type::Bitpack(b) => {
                out.push(' ');
                emit_bitpack_header(&mut out, b);
                out.push_str(" {\n");
                emit_bit_fields(&mut out, b, 2);
                out.push_str("}\n");
            }
            _ => {}
        }
    }

    for (name, import) in &ir.imports {
        out.push('\n');
        out.push_str("import ");
        out.push_str(name);
        emit_sig(&mut out, import);
        out.push_str(" {\n  symbol ");
        out.push_str(&import.symbol);
        out.push_str(";\n");
        if import.lang != Lang::C {
            out.push_str("  lang ");
            out.push_str(lang_ident(import.lang));
            out.push_str(";\n");
        }
        out.push_str("}\n");
    }

    for (name, class) in &ir.classes {
        out.push('\n');
        out.push_str("class ");
        out.push_str(name);
        out.push_str(" {\n  layout ");
        out.push_str(&class.layout);
        out.push_str(";\n");
        if let Some(base) = &class.base {
            out.push_str("  base ");
            out.push_str(base);
            out.push_str(";\n");
        }
        for (method_name, import) in &class.methods {
            out.push_str("  method ");
            out.push_str(method_name);
            emit_sig(&mut out, import);
            out.push_str(" {\n    symbol ");
            out.push_str(&import.symbol);
            out.push_str(";\n    lang ");
            out.push_str(lang_ident(import.lang));
            out.push_str(";\n  }\n");
        }
        out.push_str("}\n");
    }
    out
}

fn emit_sig(out: &mut String, import: &Import) {
    out.push('(');
    for (i, arg) in import.args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(param_token(arg));
    }
    out.push_str(") -> ");
    out.push_str(ret_token(&import.ret));
}

fn param_token(p: &Param) -> &'static str {
    match p {
        Param::Ptr => "ptr",
        Param::Len => "len",
        Param::OwnedPtr => "owned_ptr",
        Param::Scalar(s) => scalar_name(*s),
    }
}

fn ret_token(r: &Ret) -> &'static str {
    match r {
        Ret::Void => "void",
        Ret::OwnedPtr => "owned_ptr",
        Ret::Scalar(s) => scalar_name(*s),
    }
}

fn scalar_name(s: Scalar) -> &'static str {
    match s {
        Scalar::I8 => "i8",
        Scalar::U8 => "u8",
        Scalar::I16 => "i16",
        Scalar::U16 => "u16",
        Scalar::I32 => "i32",
        Scalar::U32 => "u32",
        Scalar::I64 => "i64",
        Scalar::U64 => "u64",
        Scalar::F32 => "f32",
        Scalar::F64 => "f64",
    }
}

fn emit_fields(out: &mut String, fields: &[crate::ir::Field], indent: usize) {
    let pad = " ".repeat(indent);
    for f in fields {
        out.push_str(&pad);
        out.push_str(&f.name);
        out.push_str(": ");
        emit_type(out, &f.ty, indent);
        out.push_str(";\n");
    }
}

fn emit_type(out: &mut String, ty: &Type, indent: usize) {
    match ty {
        Type::Scalar(s) => out.push_str(scalar_name(*s)),
        Type::Link => out.push_str("link"),
        Type::Array(a) => {
            out.push('[');
            emit_type(out, &a.elem, indent);
            out.push_str("; ");
            out.push_str(&a.count.to_string());
            out.push(']');
        }
        Type::Slice(s) => {
            out.push_str("slice ");
            emit_type(out, &s.elem, indent);
        }
        Type::Struct(s) => {
            out.push_str("struct repr_c {\n");
            emit_fields(out, &s.fields, indent + 2);
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        Type::Columnar(c) => {
            out.push_str("columnar(");
            out.push_str(&c.capacity.to_string());
            out.push_str(") {\n");
            emit_fields(out, &c.columns, indent + 2);
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        Type::Bitpack(b) => {
            emit_bitpack_header(out, b);
            out.push_str(" {\n");
            emit_bit_fields(out, b, indent + 2);
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
    }
}

fn emit_bitpack_header(out: &mut String, b: &BitpackType) {
    out.push_str("bitpack ");
    match b.container {
        Some(s) => out.push_str(scalar_name(s)),
        None => {
            out.push_str("span ");
            out.push_str(&b.span.to_string());
        }
    }
}

fn emit_bit_fields(out: &mut String, bp: &BitpackType, indent: usize) {
    let pad = " ".repeat(indent);
    for f in &bp.fields {
        out.push_str(&pad);
        out.push_str(&f.name);
        out.push_str(": ");
        out.push_str(&f.width.to_string());
        out.push_str(";\n");
    }
}
