use oo_gateway::query_executor::{UntrustedAdmission, UNTRUSTED_ADMISSION};

#[test]
fn untrusted_admission_stays_blocked() {
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
}
