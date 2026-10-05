//! Independent typed gold for the two-table fixture.
//!
//! The SQL file is what PostgreSQL executes. These constants are the
//! comparison values. A drift between them fails the run.

pub const MARKER: &str = "甲注评";

pub struct Customer {
    pub id: &'static str,
    pub display: Option<&'static str>,
    pub active: &'static str,
    pub private_note: Option<&'static str>,
}

pub struct Order {
    pub id: &'static str,
    pub customer_id: &'static str,
    pub memo: Option<&'static str>,
    pub posted: &'static str,
    pub amount: &'static str,
    pub clerk_note: Option<&'static str>,
}

pub const CUSTOMERS: [Customer; 3] = [
    Customer { id: "41", display: Some("丙号"), active: "true", private_note: Some("内部甲") },
    Customer { id: "42", display: None, active: "false", private_note: None },
    Customer { id: "43", display: Some("丁铺"), active: "true", private_note: Some("内部乙") },
];

pub const ORDERS: [Order; 3] = [
    Order {
        id: "701",
        customer_id: "41",
        memo: Some("首单"),
        posted: "true",
        amount: "-9223372036854775808",
        clerk_note: Some("柜员甲"),
    },
    Order {
        id: "702",
        customer_id: "41",
        memo: None,
        posted: "false",
        amount: "9223372036854775807",
        clerk_note: None,
    },
    Order {
        id: "703",
        customer_id: "43",
        memo: Some("零单"),
        posted: "true",
        amount: "0",
        clerk_note: Some("柜员乙"),
    },
];

pub const SQL_NEEDLES: [&str; 8] = [
    "丙号",
    "丁铺",
    "内部甲",
    "首单",
    "零单",
    "柜员甲",
    "-9223372036854775808",
    "9223372036854775807",
];

/// Columns a human decision classifies. `private_note` is absent on purpose.
pub const DECISIONS: [(&str, &str, &str, &str); 9] = [
    ("shop.customer", "customer_id", "none", "identifier"),
    ("shop.customer", "display_name", "personal", "person_name"),
    ("shop.customer", "active", "none", "boolean_flag"),
    ("shop.order_doc", "order_id", "none", "identifier"),
    ("shop.order_doc", "customer_id", "none", "foreign_key"),
    ("shop.order_doc", "memo", "none", "free_text"),
    ("shop.order_doc", "posted", "none", "boolean_flag"),
    ("shop.order_doc", "amount", "none", "quantity"),
    ("shop.order_doc", "clerk_note", "sensitive", "free_text"),
];

pub const CLERK_CUSTOMER: [&str; 3] = ["customer_id", "display_name", "active"];
pub const CLERK_ORDER: [&str; 6] = ["order_id", "customer_id", "memo", "posted", "amount", "clerk_note"];
pub const PEEK_CUSTOMER: [&str; 2] = ["customer_id", "display_name"];
pub const PEEK_ORDER: [&str; 3] = ["order_id", "customer_id", "posted"];
