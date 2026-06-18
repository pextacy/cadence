//! Pure median-of-N over [`U512`] — no contract env, fully unit-tested.
//!
//! Splitting this out keeps the median rule (the only non-trivial numeric logic in
//! the aggregator) testable in isolation, with checked arithmetic and no `unwrap`.

use odra::casper_types::U512;
use odra::prelude::*;

/// The median of a non-empty slice of fixed-point prices.
///
/// Returns `None` for an empty input (the caller treats too-few quotes as a quorum
/// failure). For an even count the median is the **average of the two middle
/// values**, computed with checked addition so it can never overflow `U512`.
///
/// The input is copied and sorted internally; the caller's slice is not mutated.
pub fn median_u512(values: &[U512]) -> Option<U512> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<U512> = values.to_vec();
    sorted.sort();
    let n = sorted.len();
    let mid = n / 2;
    if n % 2 == 1 {
        Some(sorted[mid])
    } else {
        let lo = sorted[mid - 1];
        let hi = sorted[mid];
        // checked_add then divide by 2: average of the two central values.
        let sum = lo.checked_add(hi)?;
        Some(sum / U512::from(2u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(n: u64) -> U512 {
        U512::from(n)
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(median_u512(&[]), None);
    }

    #[test]
    fn single_value() {
        assert_eq!(median_u512(&[u(42)]), Some(u(42)));
    }

    #[test]
    fn odd_count_picks_middle() {
        // sorted: 1, 3, 5 -> 3
        assert_eq!(median_u512(&[u(5), u(1), u(3)]), Some(u(3)));
    }

    #[test]
    fn even_count_averages_two_middle() {
        // sorted: 10, 20, 30, 40 -> (20 + 30) / 2 = 25
        assert_eq!(median_u512(&[u(40), u(10), u(30), u(20)]), Some(u(25)));
    }

    #[test]
    fn even_count_average_truncates() {
        // sorted: 1, 2 -> (1 + 2) / 2 = 1 (integer division)
        assert_eq!(median_u512(&[u(2), u(1)]), Some(u(1)));
    }

    #[test]
    fn unsorted_input_is_not_required() {
        assert_eq!(median_u512(&[u(9), u(7), u(8), u(6), u(5)]), Some(u(7)));
    }

    #[test]
    fn handles_duplicates() {
        assert_eq!(median_u512(&[u(5), u(5), u(5)]), Some(u(5)));
    }

    #[test]
    fn near_max_even_average_does_not_overflow() {
        let max = U512::MAX;
        let near = max - U512::from(2u64);
        // sorted: near, max -> (near + max)/2; checked_add must succeed here since
        // near + max < 2^512 - 1? near + max = 2*max - 2 which overflows, so None.
        assert_eq!(median_u512(&[max, near]), None);
    }

    #[test]
    fn large_even_average_within_range() {
        let a = U512::from(u128::MAX);
        let b = U512::from(u128::MAX) + U512::from(2u64);
        // (a + b)/2 = a + 1
        assert_eq!(median_u512(&[b, a]), Some(a + U512::from(1u64)));
    }
}
