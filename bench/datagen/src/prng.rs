//! `oo-splitmix64/v1`
//!
//! The mix is the SplitMix64 finalizer used by Vigna and by Java
//! `SplittableRandom` (Steele, Lea, Flood). There is no global state, no
//! `getrandom`, and no clock. A row stream is a pure function of
//! `(seed, domain, index)`.

pub const PRNG_VERSION: &str = "oo-splitmix64/v1";

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MIX_1: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_2: u64 = 0x94D0_49BB_1331_11EB;
const DOMAIN_SALT: u64 = 0x6A09_E667_F3BC_C908;

#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        mix(self.state)
    }

    pub fn gen_range(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        self.next_u64() % n
    }
}

pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(MIX_1);
    z = (z ^ (z >> 27)).wrapping_mul(MIX_2);
    z ^ (z >> 31)
}

/// Domain-separated stream. Call order across domains does not matter.
pub fn domain_rng(seed: u64, domain: &str, index: u64) -> SplitMix64 {
    let mut state = seed ^ DOMAIN_SALT;
    state = state.wrapping_add(index.wrapping_mul(GAMMA));
    for byte in domain.as_bytes() {
        state = state.wrapping_add(u64::from(*byte));
        state = mix(state);
    }
    SplitMix64::new(mix(state))
}

pub fn domain_partition(seed: u64) -> u16 {
    let mut rng = SplitMix64::new(seed);
    (rng.next_u64() >> 48) as u16
}

#[cfg(test)]
mod tests {
    use super::{SplitMix64, domain_partition, domain_rng};

    #[test]
    fn splitmix_known_vectors() {
        // Independent of this crate: Python integer mix of the published constants.
        let mut rng = SplitMix64::new(0);
        let got: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        assert_eq!(
            got,
            vec![
                0xe220_a839_7b1d_cdaf,
                0x6e78_9e6a_a1b9_65f4,
                0x06c4_5d18_8009_454f,
                0xf88b_b8a8_724c_81ec,
            ]
        );
        let mut one = SplitMix64::new(1);
        assert_eq!(one.next_u64(), 0x910a_2dec_8902_5cc1);
        let mut forty_two = SplitMix64::new(42);
        assert_eq!(forty_two.next_u64(), 0xbdd7_3226_2feb_6e95);
    }

    #[test]
    fn domain_streams_are_pure_and_separated() {
        assert_eq!(domain_rng(1, "kh", 0).next_u64(), 0x335f_78e1_9c61_4746);
        assert_eq!(domain_rng(1, "kh", 1).next_u64(), 0xc6a2_f931_28a2_0846);
        assert_eq!(domain_rng(1, "yd", 0).next_u64(), 0xc5f0_c4f2_5ea3_c037);
        assert_eq!(domain_rng(42, "gj", 7).next_u64(), 0x44a2_ac6f_8f04_a30e);
        let first = domain_rng(1, "yd", 0).next_u64();
        let _other = domain_rng(1, "kh", 0).next_u64();
        assert_eq!(domain_rng(1, "yd", 0).next_u64(), first);
    }

    #[test]
    fn partition_matches_first_mix() {
        assert_eq!(domain_partition(0), 0xe220);
        assert_eq!(domain_partition(1), 0x910a);
        assert_eq!(domain_partition(2), 0x9758);
        assert_eq!(domain_partition(42), 0xbdd7);
        assert_ne!(domain_partition(1), domain_partition(2));
    }
}
