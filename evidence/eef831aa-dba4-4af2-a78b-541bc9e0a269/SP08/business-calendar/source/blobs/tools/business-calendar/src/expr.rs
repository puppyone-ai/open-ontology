//! Bounded evaluator for anchor expressions `oo_core` has already accepted.
//!
//! The grammar matches `parse_anchor_expr`. This module executes it.
//! It does not eval a string as code, spawn a shell, or build SQL.
//! An expression the checker accepts and this parser does not understand is
//! refused. It is not replaced with a month length.

use oo_core::oos::AnchorName;

use crate::civil::{Ymd, add_days, days_from_civil};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalFail {
    TypeMismatch,
    Unsupported,
    Overflow,
    OutOfRange,
}

#[derive(Clone)]
enum Expr {
    Days(i64),
    Anchor(AnchorName),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Min(Box<Expr>, Box<Expr>),
    Max(Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy)]
enum Value {
    Date(Ymd),
    Days(i64),
}

#[derive(Default)]
pub struct AnchorValues {
    slots: [Option<Ymd>; 10],
}

impl AnchorValues {
    pub fn set(&mut self, name: AnchorName, value: Ymd) {
        self.slots[index(name)] = Some(value);
    }

    pub(crate) fn get(&self, name: AnchorName) -> Option<Ymd> {
        self.slots[index(name)]
    }
}

fn index(name: AnchorName) -> usize {
    match name {
        AnchorName::Today => 0,
        AnchorName::MonthStart => 1,
        AnchorName::PrevMonthStart => 2,
        AnchorName::PrevMonthEnd => 3,
        AnchorName::QuarterStart => 4,
        AnchorName::PrevQuarterStart => 5,
        AnchorName::PrevQuarterEnd => 6,
        AnchorName::FiscalYearStart => 7,
        AnchorName::PrevFiscalYearEnd => 8,
        AnchorName::LastClosedPeriodEnd => 9,
    }
}

pub fn eval_date(text: &str, configured: &[AnchorName], values: &AnchorValues) -> Result<Ymd, EvalFail> {
    match eval(&parse(text, configured)?, values)? {
        Value::Date(date) => Ok(date),
        Value::Days(_) => Err(EvalFail::TypeMismatch),
    }
}

fn parse(input: &str, anchors: &[AnchorName]) -> Result<Expr, EvalFail> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        index: 0,
        anchors,
    };
    let expr = parser.expr(0)?;
    parser.skip();
    if parser.index == parser.bytes.len() {
        Ok(expr)
    } else {
        Err(EvalFail::Unsupported)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    anchors: &'a [AnchorName],
}

impl<'a> Parser<'a> {
    fn expr(&mut self, depth: u16) -> Result<Expr, EvalFail> {
        if depth == 256 {
            return Err(EvalFail::Unsupported);
        }
        let mut left = self.term(depth)?;
        loop {
            self.skip();
            match self.peek() {
                Some(b'+') => {
                    self.bump();
                    let right = self.term(depth)?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Some(b'-') => {
                    self.bump();
                    let right = self.term(depth)?;
                    left = Expr::Sub(Box::new(left), Box::new(right));
                }
                _ => return Ok(left),
            }
        }
    }

    fn term(&mut self, depth: u16) -> Result<Expr, EvalFail> {
        if depth == 256 {
            return Err(EvalFail::Unsupported);
        }
        self.skip();
        if self.peek() == Some(b'(') {
            self.bump();
            let inner = self.expr(depth + 1)?;
            self.skip();
            if self.peek() != Some(b')') {
                return Err(EvalFail::Unsupported);
            }
            self.bump();
            return Ok(inner);
        }
        if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            return self.digits();
        }
        let start = self.index;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.bump();
        }
        if start == self.index {
            return Err(EvalFail::Unsupported);
        }
        let name = std::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| EvalFail::Unsupported)?;
        self.skip();
        if self.peek() == Some(b'(') {
            if name != "min" && name != "max" {
                return Err(EvalFail::Unsupported);
            }
            self.bump();
            let left = self.expr(depth + 1)?;
            self.skip();
            if self.peek() != Some(b',') {
                return Err(EvalFail::Unsupported);
            }
            self.bump();
            let right = self.expr(depth + 1)?;
            self.skip();
            if self.peek() != Some(b')') {
                return Err(EvalFail::Unsupported);
            }
            self.bump();
            return Ok(if name == "min" {
                Expr::Min(Box::new(left), Box::new(right))
            } else {
                Expr::Max(Box::new(left), Box::new(right))
            });
        }
        let anchor = self
            .anchors
            .iter()
            .copied()
            .find(|anchor| anchor.as_str() == name)
            .ok_or(EvalFail::Unsupported)?;
        Ok(Expr::Anchor(anchor))
    }

    fn digits(&mut self) -> Result<Expr, EvalFail> {
        let mut value: u64 = 0;
        let mut overflow = false;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            let digit = u64::from(self.peek().unwrap_or(b'0') - b'0');
            self.bump();
            if overflow {
                continue;
            }
            match value.checked_mul(10).and_then(|item| item.checked_add(digit)) {
                Some(next) => value = next,
                None => overflow = true,
            }
        }
        if overflow {
            return Err(EvalFail::Overflow);
        }
        let value = i64::try_from(value).map_err(|_| EvalFail::Overflow)?;
        Ok(Expr::Days(value))
    }

    fn skip(&mut self) {
        while self.peek() == Some(b' ') {
            self.bump();
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn bump(&mut self) {
        self.index += 1;
    }
}

fn eval(expr: &Expr, values: &AnchorValues) -> Result<Value, EvalFail> {
    match expr {
        Expr::Days(days) => Ok(Value::Days(*days)),
        Expr::Anchor(name) => values
            .get(*name)
            .map(Value::Date)
            .ok_or(EvalFail::Unsupported),
        Expr::Add(left, right) => add(eval(left, values)?, eval(right, values)?),
        Expr::Sub(left, right) => sub(eval(left, values)?, eval(right, values)?),
        Expr::Min(left, right) => extreme(eval(left, values)?, eval(right, values)?, true),
        Expr::Max(left, right) => extreme(eval(left, values)?, eval(right, values)?, false),
    }
}

fn add(left: Value, right: Value) -> Result<Value, EvalFail> {
    match (left, right) {
        (Value::Date(date), Value::Days(days)) | (Value::Days(days), Value::Date(date)) => {
            add_days(date, days).map(Value::Date).ok_or(EvalFail::OutOfRange)
        }
        (Value::Days(left), Value::Days(right)) => left
            .checked_add(right)
            .map(Value::Days)
            .ok_or(EvalFail::Overflow),
        (Value::Date(_), Value::Date(_)) => Err(EvalFail::TypeMismatch),
    }
}

fn sub(left: Value, right: Value) -> Result<Value, EvalFail> {
    match (left, right) {
        (Value::Date(date), Value::Days(days)) => {
            let delta = days.checked_neg().ok_or(EvalFail::Overflow)?;
            add_days(date, delta).map(Value::Date).ok_or(EvalFail::OutOfRange)
        }
        (Value::Date(left), Value::Date(right)) => {
            let left_days = days_from_civil(left.year, left.month, left.day).ok_or(EvalFail::OutOfRange)?;
            let right_days = days_from_civil(right.year, right.month, right.day).ok_or(EvalFail::OutOfRange)?;
            left_days
                .checked_sub(right_days)
                .map(Value::Days)
                .ok_or(EvalFail::Overflow)
        }
        (Value::Days(left), Value::Days(right)) => left
            .checked_sub(right)
            .map(Value::Days)
            .ok_or(EvalFail::Overflow),
        (Value::Days(_), Value::Date(_)) => Err(EvalFail::TypeMismatch),
    }
}

fn extreme(left: Value, right: Value, lesser: bool) -> Result<Value, EvalFail> {
    match (left, right) {
        (Value::Date(left), Value::Date(right)) => Ok(Value::Date(if lesser == (left <= right) { left } else { right })),
        (Value::Days(left), Value::Days(right)) => Ok(Value::Days(if lesser == (left <= right) { left } else { right })),
        _ => Err(EvalFail::TypeMismatch),
    }
}

#[cfg(test)]
mod tests {
    use oo_core::oos::AnchorName;

    use super::{AnchorValues, EvalFail, eval_date};
    use crate::civil::Ymd;

    fn values(today: Ymd, month: Ymd, prev: Ymd) -> (AnchorValues, Vec<AnchorName>) {
        let mut values = AnchorValues::default();
        values.set(AnchorName::Today, today);
        values.set(AnchorName::MonthStart, month);
        values.set(AnchorName::PrevMonthStart, prev);
        (
            values,
            vec![
                AnchorName::Today,
                AnchorName::MonthStart,
                AnchorName::PrevMonthStart,
            ],
        )
    }

    #[test]
    fn month_to_date_clamp() {
        let cases = [
            ((2026, 3, 31), (2026, 3, 1)),
            ((2026, 3, 15), (2026, 2, 16)),
            ((2024, 3, 29), (2024, 3, 1)),
            ((2024, 3, 30), (2024, 3, 1)),
        ];
        for ((year, month, day), (ey, em, ed)) in cases {
            let today = Ymd { year, month, day };
            let month_start = Ymd { year, month, day: 1 };
            let prev = if month == 1 {
                Ymd { year: year - 1, month: 12, day: 1 }
            } else {
                Ymd { year, month: month - 1, day: 1 }
            };
            let (values, configured) = values(today, month_start, prev);
            let got = eval_date(
                "min(month_start, prev_month_start + (today - month_start) + 1)",
                &configured,
                &values,
            )
            .unwrap();
            assert_eq!(got, Ymd { year: ey, month: em, day: ed });
        }
    }

    #[test]
    fn day_count_is_not_a_date_and_max_is_later() {
        let (values, configured) = values(
            Ymd { year: 2026, month: 3, day: 15 },
            Ymd { year: 2026, month: 3, day: 1 },
            Ymd { year: 2026, month: 2, day: 1 },
        );
        assert_eq!(
            eval_date("today - month_start", &configured, &values),
            Err(EvalFail::TypeMismatch)
        );
        assert_eq!(
            eval_date("max(today, month_start)", &configured, &values).unwrap(),
            Ymd { year: 2026, month: 3, day: 15 }
        );
        assert_eq!(
            eval_date("today - 1 - 1", &configured, &values).unwrap(),
            Ymd { year: 2026, month: 3, day: 13 }
        );
    }
}
