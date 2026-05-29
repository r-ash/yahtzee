// ============================================================================
// SECTION 1: DICE REPRESENTATION
// ============================================================================
// A roll of 5 six-sided dice is a *multiset* — we only care about
// which values appeared, not which physical die showed what.
// We represent a roll as [u8; 6] where counts[i] = number of (i+1)-pips.
// E.g., rolling 1,1,3,3,3 → [2,0,3,0,0,0]
//
// There are C(10,5) = 252 distinct rolls of 5 dice.
// A "keep" is any sub-multiset: counts[i] <= roll[i] for all i.

use crate::state::{CHANCE_CAT, FOUR_OF_A_KIND_CAT, FULL_HOUSE_CAT, LRG_STRAIGHT_CAT, ONES_CAT, SIXES_CAT, SML_STRAIGHT_CAT, THREE_OF_A_KIND_CAT, YAHTZEE_CAT};

pub type Roll = [u8; 6];
pub const NUM_ROLLS: usize = 252;

fn sum_dice(roll: &Roll) -> u32 {
    return roll
        .iter()
        .enumerate()
        .fold(0u32, |sum, (idx, &n_dice)| sum + ((n_dice as u32) * (idx + 1) as u32));
}

fn get_straight_length(roll: &Roll) -> usize {
    let mut max_len = 0;
    let mut current_len = 0;

    roll.iter().for_each(|&n_dice| {
        if n_dice >= 1 {
            current_len += 1;
        } else {
            if current_len > max_len {
                max_len = current_len
            }
            current_len = 0
        }
    });

    max_len.max(current_len)
}

/// Generate all multisets of `n` dice from faces {1..6}.
/// Uses stars-and-bars recursion over face indices.
pub fn enumerate_rolls(n: u8) -> Vec<Roll> {
    let mut result = Vec::new();
    fn helper(n: u8, face: usize, current: Roll, out: &mut Vec<Roll>) {
        if face == 6 {
            if n == 0 { out.push(current); }
            return;
        }
        for k in 0..=n {
            let mut next = current;
            next[face] = k;
            helper(n - k, face + 1, next, out);
        }
    }
    helper(n, 0, [0; 6], &mut result);
    result
}

// Get all subsets of "roll" these are all our possible choices
// for keeps
pub fn enumerate_keeps(roll: &Roll) -> Vec<Roll> {
    let mut result = Vec::new();
    fn helper(roll: &Roll, face: usize, current: Roll, out: &mut Vec<Roll>) {
        if face == 6 {
            out.push(current);
            return;
        }
        for k in 0..=roll[face] {
            let mut next = current;
            next[face] = k;
            helper(roll, face + 1, next, out);
        }
    }
    helper(roll, 0, [0; 6], &mut result);
    result
}

pub fn score_roll(roll: &Roll, category: u8, yahtzee_bonus: bool) -> u32 {
    match category {
        ONES_CAT..=SIXES_CAT => {
            return (roll[category as usize] * (category + 1)) as u32
        },
        THREE_OF_A_KIND_CAT => {
            if roll.iter().any(|&c| c >= 3) {
                return sum_dice(roll);
            } else {
                return 0;
            }
        },
        FOUR_OF_A_KIND_CAT => {
            if roll.iter().any(|&c| c >= 4) {
                return sum_dice(roll);
            } else {
                return 0;
            }
        },
        FULL_HOUSE_CAT => {
            if roll.contains(&2) && roll.contains(&3) {
                return 25
            } else {
                return 0
            }
        },
        SML_STRAIGHT_CAT => {
            if get_straight_length(roll) >= 4 {
                return 30
            } else {
                return 0
            }
        },
        LRG_STRAIGHT_CAT => {
            if get_straight_length(roll) == 5 {
                return 40
            } else {
                return 0
            }
        }
        YAHTZEE_CAT => {
            if roll.iter().any(|&c| c == 5) {
                if yahtzee_bonus {
                    return 100
                } else {
                    return 50
                }
            } else {
                return 0;
            }
        },
        CHANCE_CAT => {
            return sum_dice(roll)
        },
        _ => panic!("Recevied invalid category to score")
    }
}

// P(keep → full_roll): probability that rerolling the un-kept dice yields `full_roll`.
/// If `full_roll` does not contain `keep` as a sub-multiset, returns 0.
/// Otherwise: multinomial(n_rerolled; d[0],...,d[5]) / 6^n_rerolled
/// where d[i] = full_roll[i] - keep[i].
pub fn transition_prob(keep: &Roll, target: &Roll) -> f64 {

    let mut diff = [0i32; 6];
    let mut n_reroll = 0i32;
    for i in 0..6 {
        diff[i] = target[i] as i32 - keep[i] as i32;
        if diff[i] < 0 { return 0.0; }
        n_reroll += diff[i];
    }
    // We have a list of dice we want to get
    // what is the prob of getting this by rolling that many dice
    // if n is number of dice and n1, n2, .. are number of 
    // unique faces e.g. rolling 3 dice to get 1, 2, 2 we
    // have n = 3, n1= 1, n2 = 2, or if rolling 3 dice
    // to get 1, 1, 1, n = 3, n1 = 3. Then it is
    // n! / (n1! * n2! ... * ni!)
    // We can compute this incrementally to avoid factorials
    // getting really big.
    // = n_reroll! / (d[0]! * ... * d[5]!) / 6^n_reroll
    let mut prob = 1.0f64;
    let mut placed = 0i32;
    for i in 0..6 {
        for j in 0..diff[i] {
            prob *= (n_reroll - placed - j) as f64;
            prob /= (j + 1) as f64;
            prob /= 6.0;
        }
        placed += diff[i];
    }
    prob
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_list_all_rolls() {
        let rolls = enumerate_rolls(5);
        assert_eq!(rolls.len(), 252)
    }

    #[test]
    fn test_can_list_all_keep() {
        let keeps = enumerate_keeps(&[5, 0, 0, 0, 0, 0]);
        assert_eq!(keeps.len(), 6); // 0, 1, 2, .. 5

        let keeps = enumerate_keeps(&[2, 3, 0, 0, 0, 0]);
        assert_eq!(keeps.len(), 12); // (0, 0), (1, 0), (2, 0), (0, 1) .. 3 * 4

        let keeps = enumerate_keeps(&[2, 1, 2, 0, 0, 0]);
        assert_eq!(keeps.len(), 18); // 3 * 2 * 3
    }

    #[test]
    fn test_can_get_straight_length() {
        assert_eq!(get_straight_length(&[1, 1, 1, 1, 1, 0]), 5);
        assert_eq!(get_straight_length(&[1, 2, 1, 1, 0, 0]), 4);
        assert_eq!(get_straight_length(&[0, 2, 1, 1, 1, 0]), 4);
        assert_eq!(get_straight_length(&[0, 2, 1, 1, 0, 1]), 3);
        assert_eq!(get_straight_length(&[0, 2, 0, 1, 0, 2]), 1);
        assert_eq!(get_straight_length(&[0, 5, 0, 0, 0, 0]), 1);
    }

    #[test]
    fn test_can_score_rolls() {
        // TODO: make a test table macro?
        assert_eq!(
            score_roll(&[1, 1, 1, 1, 1, 0], ONES_CAT, false),
            1
        );
        assert_eq!(
            score_roll(&[5, 0, 0, 0, 0, 0], ONES_CAT, false),
            5
        );
        assert_eq!(
            score_roll(&[0, 5, 0, 0, 0, 0], ONES_CAT, false),
            0
        );
        assert_eq!(
            score_roll(&[0, 5, 0, 0, 0, 0], TWOS_CAT, false),
            5 * 2
        );

        assert_eq!(
            score_roll(&[0, 5, 0, 0, 0, 0], THREE_OF_A_KIND_CAT, false),
            5 * 2
        );
        assert_eq!(
            score_roll(&[0, 3, 0, 2, 0, 0], THREE_OF_A_KIND_CAT, false),
            2 * 3 + 4 * 2
        );
        assert_eq!(
            score_roll(&[0, 2, 0, 2, 1, 0], THREE_OF_A_KIND_CAT, false),
            0
        );

        assert_eq!(
            score_roll(&[0, 5, 0, 0, 0, 0], FOUR_OF_A_KIND_CAT, false),
            5 * 2
        );
        assert_eq!(
            score_roll(&[0, 4, 0, 1, 0, 0], FOUR_OF_A_KIND_CAT, false),
            2 * 4 + 4 * 1
        );
        assert_eq!(
            score_roll(&[0, 3, 0, 2, 0, 0], FOUR_OF_A_KIND_CAT, false),
            0
        );

        assert_eq!(
            score_roll(&[0, 3, 0, 2, 0, 0], FULL_HOUSE_CAT, false),
            25
        );
        assert_eq!(
            score_roll(&[0, 2, 1, 2, 0, 0], FULL_HOUSE_CAT, false),
            0
        );

        assert_eq!(
            score_roll(&[1, 1, 1, 1, 1, 0], SML_STRAIGHT_CAT, false),
            30
        );
        assert_eq!(
            score_roll(&[1, 1, 2, 1, 0, 0], SML_STRAIGHT_CAT, false),
            30
        );
        assert_eq!(
            score_roll(&[0, 0, 1, 2, 1, 1], SML_STRAIGHT_CAT, false),
            30
        );
        assert_eq!(
            score_roll(&[0, 1, 2, 1, 1, 0], SML_STRAIGHT_CAT, false),
            30
        );
        assert_eq!(
            score_roll(&[2, 1, 0, 1, 1, 0], SML_STRAIGHT_CAT, false),
            0
        );

        assert_eq!(
            score_roll(&[1, 1, 1, 1, 1, 0], LRG_STRAIGHT_CAT, false),
            40
        );
        assert_eq!(
            score_roll(&[0, 1, 1, 1, 1, 1], LRG_STRAIGHT_CAT, false),
            40
        );
        assert_eq!(
            score_roll(&[1, 1, 0, 1, 1, 1], LRG_STRAIGHT_CAT, false),
            0
        );

        assert_eq!(
            score_roll(&[5, 0, 0, 0, 0, 0], YAHTZEE_CAT, false),
            50
        );
        assert_eq!(
            score_roll(&[5, 0, 0, 0, 0, 0], YAHTZEE_CAT, true),
            100
        );
        assert_eq!(
            score_roll(&[4, 1, 0, 0, 0, 0], YAHTZEE_CAT, true),
            0
        );

        assert_eq!(
            score_roll(&[5, 0, 0, 0, 0, 0], CHANCE_CAT, false),
            1 * 5
        );
        assert_eq!(
            score_roll(&[3, 0, 1, 0, 1, 0], CHANCE_CAT, false),
            1 * 3 + 3 * 1 + 5 * 1
        );
    }

    #[test]
    fn test_transition_probability() {
        // Hard to verify everything here but we can check some obvious cases
        assert_eq!(
            transition_prob(&[0, 0, 0, 0, 0, 0], &[5, 0, 0, 0, 0, 0]),
            1.0 / 7776.0
        );
        assert_eq!(
            transition_prob(&[5, 0, 0, 0, 0, 0], &[0, 5, 0, 0, 0, 0]),
            0.0
        );
        assert_eq!(
            transition_prob(&[4, 0, 0, 0, 0, 0], &[5, 0, 0, 0, 0, 0]),
            1.0 / 6.0
        );
    }
    
}