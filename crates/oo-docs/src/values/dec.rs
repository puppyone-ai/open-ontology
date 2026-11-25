//! Exact decimals: `m * 10^e` with `m` bounded, so no value passes through a float. Pure.

/// Largest mantissa (30 digits).
const MAX_M: u128 = 1_000_000_000_000_000_000_000_000_000_000;
/// Largest plain rendering (digits before the point).
const MAX_PLAIN: i32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Dec {
    m: u128,
    e: i32,
}

impl Dec {
    pub const ZERO: Dec = Dec { m: 0, e: 0 };

    pub fn new(mut m: u128, mut e: i32) -> Option<Dec> {
        if m == 0 {
            return Some(Dec::ZERO);
        }
        while m.is_multiple_of(10) {
            m /= 10;
            e += 1;
        }
        (m < MAX_M && (-30..=30).contains(&e)).then_some(Dec { m, e })
    }

    pub fn from_u64(n: u64) -> Dec {
        Dec::new(u128::from(n), 0).unwrap_or(Dec::ZERO)
    }

    pub fn is_zero(&self) -> bool {
        self.m == 0
    }

    pub fn is_integer(&self) -> bool {
        self.e >= 0 || self.m == 0
    }

    /// Digits `[0-9]+(.[0-9]+)?` with optional `,` thousands groups of exactly three digits.
    pub fn parse_ascii(s: &str) -> Option<Dec> {
        let (int, frac) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s, ""),
        };
        if int.is_empty() || !int.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        let mut digits = String::new();
        if int.contains(',') {
            let parts: Vec<&str> = int.split(',').collect();
            if parts[0].is_empty()
                || parts[0].len() > 3
                || parts[1..].iter().any(|p| p.len() != 3)
                || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
            {
                return None;
            }
            parts.iter().for_each(|p| digits.push_str(p));
        } else {
            if !int.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.push_str(int);
        }
        if s.contains('.') && (frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        digits.push_str(frac);
        if digits.len() > 30 {
            return None;
        }
        Dec::new(digits.parse().ok()?, -(frac.len() as i32))
    }

    pub fn times(self, o: Dec) -> Option<Dec> {
        Dec::new(self.m.checked_mul(o.m)?, self.e + o.e)
    }

    pub fn times_u128(self, n: u128) -> Option<Dec> {
        self.times(Dec::new(n, 0)?)
    }

    pub fn pow10(self, k: i32) -> Option<Dec> {
        Dec::new(self.m, self.e + k)
    }

    fn aligned(self, o: Dec) -> Option<(u128, u128, i32)> {
        let e = self.e.min(o.e);
        let a = self
            .m
            .checked_mul(10u128.checked_pow((self.e - e) as u32)?)?;
        let b = o.m.checked_mul(10u128.checked_pow((o.e - e) as u32)?)?;
        Some((a, b, e))
    }

    pub fn plus(self, o: Dec) -> Option<Dec> {
        let (a, b, e) = self.aligned(o)?;
        Dec::new(a.checked_add(b)?, e)
    }

    pub fn cmp_dec(self, o: Dec) -> Option<std::cmp::Ordering> {
        let (a, b, _) = self.aligned(o)?;
        Some(a.cmp(&b))
    }

    /// The integer value when it is whole and fits.
    pub fn to_u64(self) -> Option<u64> {
        if self.m == 0 {
            return Some(0);
        }
        if self.e < 0 {
            return None;
        }
        u64::try_from(self.m.checked_mul(10u128.checked_pow(self.e as u32)?)?).ok()
    }

    /// Plain decimal text: no exponent, no grouping, no trailing zeros (`6850000`, `0.12`, `3.5`).
    pub fn to_plain(self) -> String {
        if self.m == 0 {
            return "0".to_owned();
        }
        let digits = self.m.to_string();
        if self.e >= 0 {
            let z = self.e.min(MAX_PLAIN) as usize;
            return format!("{digits}{}", "0".repeat(z));
        }
        let k = (-self.e) as usize;
        if digits.len() > k {
            let (i, f) = digits.split_at(digits.len() - k);
            format!("{i}.{f}")
        } else {
            format!("0.{}{digits}", "0".repeat(k - digits.len()))
        }
    }

    /// Integer part grouped by thousands (`6,850,000`), the name the anchors use.
    pub fn to_grouped(self) -> Option<String> {
        let n = self.to_u64()?;
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        Some(out)
    }
}

impl std::fmt::Display for Dec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_plain())
    }
}
