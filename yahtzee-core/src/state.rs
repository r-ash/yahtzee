use crate::rolls;

// ============================================================================
// SECTION 3: STATE ENCODING
// ============================================================================
// From the paper, Section 4:
//   - Track which categories are used: 2^13 = 8192 possibilities (bitmask)
//   - Track upper section total: 0..63 capped (64 values = 6 bits)
//   - Track Yahtzee bonus flag: 1 bit
// Total: 13 + 6 + 1 = 20 bits → table size 2^20 = 1,048,576 entries (~8MB of f64)
//
// Encoding: [bits 0-12] = categories | [bits 13-18] = upper_total | [bit 19] = yahtzee_bonus

pub const TABLE_SIZE: usize = 1 << 20;

const CATEGORIES_BIT_MASK: usize = 0b0001_1111_1111_1111;
pub const NUM_CATEGORIES: u8 = 13;
pub const FILLED_CATEGORIES: u16 =  (1 << NUM_CATEGORIES) - 1;
pub const ONES_CAT: u8 = 0;
pub const TWOS_CAT: u8 = 1;
pub const THREES_CAT: u8 = 2;
pub const SIXES_CAT: u8 = 5;
pub const THREE_OF_A_KIND_CAT: u8 = 6;
pub const FOUR_OF_A_KIND_CAT: u8 = 7;
pub const FULL_HOUSE_CAT: u8 = 8;
pub const SML_STRAIGHT_CAT: u8 = 9;
pub const LRG_STRAIGHT_CAT: u8 = 10;
pub const YAHTZEE_CAT: u8 = 11;
pub const CHANCE_CAT: u8 = 12;

pub fn encode_state(categories: u16, upper_total: u8, yahtzee: bool) -> usize {
    (categories as usize) | ((upper_total as usize) << 13) | ((yahtzee as usize) << 19) 
}

pub fn decode_state(idx: usize) -> (u16, u8, bool) {
    let categories = (idx & CATEGORIES_BIT_MASK) as u16;
    let upper_total = ((idx >> 13) & 0x3F) as u8;
    let yahtzee_bonus = (idx >> 19) & 1 == 1;
    (categories, upper_total, yahtzee_bonus)
}

pub fn next_state(categories: u16, upper_total: u8, yahtzee_bonus: bool, roll: &rolls::Roll, category: u8) -> usize {
    let mut new_categories = categories;
    let mut new_upper = upper_total;
    let mut bonus = yahtzee_bonus;
    new_categories |= 1 << category;
    if category < 6 {
        // Use u16 to avoid overflow: max addition is 5 sixes = 30; max upper_total = 63+30 = 93
        let added = roll[category as usize] as u16 * (category as u16 + 1);
        new_upper = ((upper_total as u16 + added).min(63)) as u8;
    }
    // Once you score Yahtzee with 50, future Yahtzees earn the bonus
    if category == YAHTZEE_CAT && roll.iter().any(|&c| c == 5) {
        bonus = true;
    }

    encode_state(new_categories, new_upper, bonus)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_encode_and_decode_state() {
        let categories: u16 = 0b000_0100_0010_1011;
        let upper_total: u8 = 23;
        let yahtzee: bool = true;

        let encoded = encode_state(categories, upper_total, yahtzee);

        let (cat_decoded, upp_total_decoded, yahtzee_decoded) = decode_state(encoded);

        assert_eq!(cat_decoded, categories);
        assert_eq!(upp_total_decoded, upper_total);
        assert_eq!(yahtzee_decoded, yahtzee);
    }

    #[test]
    fn test_can_get_next_state() {
        let categories: u16 = 0b000_0100_0010_1010;
        let upper_total: u8 = 23;
        let yahtzee: bool = true;

        // fill all 1s in "1"s category
        let next = next_state(categories, upper_total, yahtzee, &[5, 0, 0, 0, 0, 0], ONES_CAT);

        let (cat_decoded, upp_total_decoded, yahtzee_decoded) = decode_state(next);

        assert_eq!(cat_decoded, 0b000_0100_0010_1011);
        assert_eq!(upp_total_decoded, upper_total + 5);
        assert_eq!(yahtzee_decoded, yahtzee);

        // fill all 1s in "3s" category
        let next2 = next_state(cat_decoded, upp_total_decoded, yahtzee_decoded, &[5, 0, 0, 0, 0, 0], THREES_CAT);
        let (cat_decoded2, upp_total_decoded2, yahtzee_decoded2) = decode_state(next2);

        assert_eq!(cat_decoded2, 0b000_0100_0010_1111);
        assert_eq!(upp_total_decoded2, upp_total_decoded);
        assert_eq!(yahtzee_decoded2, yahtzee);
    }

    #[test]
    fn test_yahtzee_bonus_set_next_state() {
        let categories: u16 = 0b000_0100_0010_1010;
        let upper_total: u8 = 23;
        let yahtzee: bool = false;

        let next = next_state(categories, upper_total, yahtzee, &[5, 0, 0, 0, 0, 0], YAHTZEE_CAT);

        let (cat_decoded, upp_total_decoded, yahtzee_decoded) = decode_state(next);

        assert_eq!(cat_decoded, 0b000_1100_0010_1010);
        assert_eq!(upp_total_decoded, upper_total);
        assert_eq!(yahtzee_decoded, true);
    }
}