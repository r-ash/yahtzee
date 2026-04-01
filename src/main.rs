use std::time::Instant;

use crate::state::NUM_CATEGORIES;

mod state;
mod rolls;

pub struct Solver {
    e_state: Vec<f64>,
    all_rolls: Vec<rolls::Roll>,
    trans_from_empty: Vec<f64>,
    // For each roll index: list of keeps, each as a sparse list of (target_roll_idx, probability).
    // Precomputed once to avoid recomputing transition_prob and enumerate_keeps in the hot loop.
    precomputed_keep_trans: Vec<Vec<Vec<(usize, f64)>>>,
    // score_table[roll_idx][yahtzee_bonus as usize][category] — avoids recomputing score_roll
    // in the hot loop (called 252 * 13 times per state).
    score_table: Vec<[[u32; 13]; 2]>,
}

impl Solver {
    pub fn new() -> Self {
        let all_rolls = rolls::enumerate_rolls(5);
        let empty: rolls::Roll = [0; 6];
        let trans_from_empty: Vec<f64> = all_rolls
            .iter()
            .map(|r| rolls::transition_prob(&empty, r))
            .collect();

        let precomputed_keep_trans: Vec<Vec<Vec<(usize, f64)>>> = all_rolls
            .iter()
            .map(|roll| {
                rolls::enumerate_keeps(roll)
                    .iter()
                    .map(|keep| {
                        all_rolls.iter()
                            .enumerate()
                            .filter_map(|(target_idx, target)| {
                                let p = rolls::transition_prob(keep, target);
                                if p > 0.0 { Some((target_idx, p)) } else { None }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();

        let score_table: Vec<[[u32; 13]; 2]> = all_rolls.iter()
            .map(|roll| [
                std::array::from_fn(|c| rolls::score_roll(roll, c as u8, false)),
                std::array::from_fn(|c| rolls::score_roll(roll, c as u8, true)),
            ])
            .collect();

        Self {
            e_state: vec![0.0; state::TABLE_SIZE],
            all_rolls,
            trans_from_empty,
            precomputed_keep_trans,
            score_table,
        }
    }

    pub fn solve(&mut self) {

        // Initialize state, all categories filled, upper values 0 to 63 and yahtzee bonus true/false
        (0u8..=63).for_each(|upper_total| {
            [false, true].into_iter().for_each(|yahtzee| {
                let state = state::encode_state(state::FILLED_CATEGORIES, upper_total, yahtzee);
                self.e_state[state] = if upper_total >= 63 { 35.0 } else { 0.0 };
            })
        });

        // Step backwards, to 12 categories filled, 11, ...
        // do this by listing all categories with n filled, all upper totals, and all yahtzee bonuses
        // TODO: can we exclude impossible combinations here?
        (0..state::NUM_CATEGORIES).rev().for_each(|n_filled| {
            println!("Working back through categories, at stage {n_filled}");
            // TODO, can we remove this inner categories loop?
            for categories in 0u16..(1 << state::NUM_CATEGORIES) {
                let n_filled_u32 = n_filled as u32;
                if categories.count_ones() != n_filled_u32 { continue; }
                // TODO: this is many many impossible states, can we refine this somehow?
                // think about possible states?
                (0u8..=63).for_each(|upper_total| {
                    [false, true].into_iter().for_each(|yahtzee| {
                        let state = state::encode_state(categories, upper_total, yahtzee);
                        let value = self.compute_widget_e_value(categories, upper_total, yahtzee);
                        self.e_state[state] = value;
                    })
                });
            }
        });
    }

    fn compute_widget_e_value(&self, categories: u16, upper_total: u8, yahtzee_bonus: bool) -> f64 {

        // Step A: For each possible final roll r, compute E(S, r, 0)
        // = best score from choosing a category to fill
        let expected_after_last_roll: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_category_value(categories, upper_total, yahtzee_bonus, i)
        );

        // Step B: For each keep r', compute E(S, r', 1)
        // = expected value over all possible reroll outcomes, each evaluated by E(S, r'', 0)
        // Then Step C: E(S, r, 1) = max over r' ⊆ r of E(S, r', 1)
        let expected_one_roll_remaining: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_keep_expected(i, &expected_after_last_roll)
        );

        // Step D/E: Same thing one level up for the first keep decision
        let expected_two_rolls_remaining: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_keep_expected(i, &expected_one_roll_remaining)
        );
 
        // Step F: E(S) = Σ_r P(∅ → r) * E(S, r, 2)
        // (the initial roll — no dice kept yet)
        self.trans_from_empty
            .iter()
            .zip(expected_two_rolls_remaining.iter())
            .map(|(p, e)| p * e)
            .sum()
    }

    /// Given a current state and a roll index, get the best category to put this
    /// roll into. Best category is the max of score of this roll + expected of next state
    fn best_category_value(&self, categories: u16, upper_total: u8, yahtzee_bonus: bool, roll_idx: usize) -> f64 {
        let roll = &self.all_rolls[roll_idx];
        let scores = &self.score_table[roll_idx][yahtzee_bonus as usize];
        let mut best = 0.0;
        (0..NUM_CATEGORIES).for_each(|c| {
            if categories & (1 << c) == 0 {
                let sc = scores[c as usize] as f64;
                let ns = state::next_state(categories, upper_total, yahtzee_bonus, roll, c);
                let val = sc + self.e_state[ns];
                if val > best { best = val; }
            }
        });

        return best
    }

    /// Given a roll index and a table of values for each full roll outcome,
    /// find the best keep r' ⊆ r and return its expected value.
    ///
    /// E(S, r, n) = max_{r' ⊆ r} Σ_{r''} P(r' → r'') * e_next[r'']
    fn best_keep_expected(&self, roll_idx: usize, e_next: &[f64]) -> f64 {
        self.precomputed_keep_trans[roll_idx]
            .iter()
            .map(|keep_trans| {
                keep_trans.iter()
                    .map(|&(target_idx, prob)| prob * e_next[target_idx])
                    .sum::<f64>()
            })
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

fn main() {
    println!("\nSolving... (this takes a few minutes)");
    let t0 = Instant::now();
    let mut solver = Solver::new();
    solver.solve();
    println!("Solved in {:.1}s", t0.elapsed().as_secs_f64());

    let initial = state::encode_state(0, 0, false);
    println!("\nExpected score under optimal play: {:.4}", solver.e_state[initial]);
    println!("(Glenn reports 254.59 with Yahtzee bonuses; 245.87 without)");
}
