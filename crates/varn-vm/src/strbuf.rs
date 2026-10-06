








use std::fmt;



pub const INT_MAX_DIGITS: usize = 20;





pub(crate) fn itoa(v: i64, buf: &mut [u8; INT_MAX_DIGITS]) -> &str {
    let negative = v < 0;
    
    
    let mut n = if negative {
        (v as u64).wrapping_neg()
    } else {
        v as u64
    };

    let mut i = INT_MAX_DIGITS;
    if n == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
    }
    if negative {
        i -= 1;
        buf[i] = b'-';
    }

    
    std::str::from_utf8(&buf[i..]).expect("itoa writes ASCII only")
}



pub const INLINE_CAP: usize = 64;


pub struct StrBuf {
    inline: [u8; INLINE_CAP],
    len: usize,
    spilled: Option<String>,
}

impl Default for StrBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl StrBuf {
    pub(crate) fn new() -> Self {
        Self {
            inline: [0; INLINE_CAP],
            len: 0,
            spilled: None,
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        match &self.spilled {
            Some(s) => s,
            
            
            None => std::str::from_utf8(&self.inline[..self.len])
                .expect("StrBuf only ever appends whole &str"),
        }
    }

    pub(crate) fn push_str(&mut self, s: &str) {
        if let Some(spilled) = &mut self.spilled {
            spilled.push_str(s);
            return;
        }
        if self.len + s.len() <= INLINE_CAP {
            self.inline[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
            return;
        }
        
        
        let mut spilled = String::with_capacity((self.len + s.len()) * 2);
        spilled.push_str(
            std::str::from_utf8(&self.inline[..self.len])
                .expect("StrBuf only ever appends whole &str"),
        );
        spilled.push_str(s);
        self.spilled = Some(spilled);
    }

    
    
    pub(crate) fn into_string(self) -> String {
        match self.spilled {
            Some(s) => s,
            None => self.as_str().to_owned(),
        }
    }
}

impl fmt::Write for StrBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
}
