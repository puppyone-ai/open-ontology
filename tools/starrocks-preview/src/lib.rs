//! StarRocks controlled-preview source product.

#![forbid(unsafe_code)]

pub mod admission;
pub mod catalog;
pub mod import;
pub mod names;
pub mod product;

pub const CONFIG_PROTOCOL: &str = "oo.starrocks-preview.config/v1";
