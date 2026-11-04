use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Name(String),
    Index(u64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Path {
    pub segs: Vec<Seg>,
}

impl Path {
    pub fn new() -> Self {
        Self { segs: Vec::new() }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.segs.push(Seg::Name(name.into()));
        self
    }

    pub fn index(mut self, index: u64) -> Self {
        self.segs.push(Seg::Index(index));
        self
    }
}

impl From<Vec<Seg>> for Path {
    fn from(segs: Vec<Seg>) -> Self {
        Self { segs }
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segs.is_empty() {
            return write!(f, "$");
        }
        let mut first = true;
        for seg in &self.segs {
            match seg {
                Seg::Name(n) => {
                    if !first {
                        write!(f, ".")?;
                    }
                    write!(f, "{n}")?;
                    first = false;
                }
                Seg::Index(i) => {
                    write!(f, "[{i}]")?;
                    first = false;
                }
            }
        }
        Ok(())
    }
}
