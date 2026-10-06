//! Exact declared price. This is not a billing certificate.

use crate::error::ProviderError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Price {
    units: u128,
    scale: u32,
    lexeme: String,
}

impl Price {
    pub(crate) fn parse(text: &str) -> Result<Self, ProviderError> {
        if text.is_empty() || text.len() > 32 || text.starts_with('+') || text.contains(['e', 'E', '-'])
        {
            return Err(ProviderError::new(
                "config.price",
                "the declared price is not an allowed decimal",
            ));
        }
        let (whole, frac) = match text.split_once('.') {
            Some((whole, frac)) => (whole, frac),
            None => (text, ""),
        };
        if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ProviderError::new(
                "config.price",
                "the declared price is not an allowed decimal",
            ));
        }
        if frac.len() > 8 || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ProviderError::new(
                "config.price",
                "the declared price is not an allowed decimal",
            ));
        }
        let scale = u32::try_from(frac.len()).unwrap_or(9);
        let digits = if frac.is_empty() {
            whole.to_owned()
        } else {
            format!("{whole}{frac}")
        };
        let units = digits.parse::<u128>().map_err(|_| {
            ProviderError::new(
                "config.price",
                "the declared price is not an allowed decimal",
            )
        })?;
        Ok(Self {
            units,
            scale,
            lexeme: text.to_owned(),
        })
    }

    pub(crate) fn lexeme(&self) -> &str {
        &self.lexeme
    }

    /// `tokens` times this price, at the same fractional scale.
    pub(crate) fn amount(&self, tokens: u64) -> Result<String, ProviderError> {
        let product = u128::from(tokens).checked_mul(self.units).ok_or_else(|| {
            ProviderError::new("config.price", "the declared amount does not fit")
        })?;
        Ok(format_units(product, self.scale))
    }
}

fn format_units(units: u128, scale: u32) -> String {
    if scale == 0 {
        return units.to_string();
    }
    let factor = 10u128.pow(scale);
    let whole = units / factor;
    let frac = units % factor;
    format!("{whole}.{frac:0width$}", width = scale as usize)
}

#[cfg(test)]
mod tests {
    use super::Price;

    #[test]
    fn multiplies_without_rounding() {
        let price = Price::parse("0.000002").expect("price");
        assert_eq!(price.amount(18).expect("amount"), "0.000036");
    }
}
