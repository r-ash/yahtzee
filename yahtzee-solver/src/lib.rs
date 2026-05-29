use yahtzee_core::{rolls, state};
use yahtzee_core::state::NUM_CATEGORIES;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

pub struct Solver {
    pub e_state: Vec<f64>,
    all_rolls: Vec<rolls::Roll>,
    trans_from_empty: Vec<f64>,
    // For each roll index: list of keeps, each as a sparse list of (target_roll_idx, probability).
    // Precomputed once to avoid recomputing transition_prob and enumerate_keeps in the hot loop.
    precomputed_keep_trans: Vec<Vec<Vec<(usize, f64)>>>,
    // score_table[roll_idx][yahtzee_bonus as usize][category] — avoids recomputing score_roll
    // in the hot loop (called 252 * 13 times per state).
    score_table: Vec<[[u32; 13]; 2]>,
}

pub enum Advice {
    FillCategory(u8),
    KeepDice(rolls::Roll),
}

impl Solver {
    /// Create a Solver with a pre-computed e_state (e.g. loaded from disk for the API).
    /// All precomputed lookup tables are built fresh; only e_state is taken from the caller.
    pub fn from_e_state(e_state: Vec<f64>) -> Self {
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
            e_state,
            all_rolls,
            trans_from_empty,
            precomputed_keep_trans,
            score_table,
        }
    }

    /// Create a fresh Solver ready to be solved.
    pub fn new() -> Self {
        Self::from_e_state(vec![0.0; state::TABLE_SIZE])
    }

    /// Run the full dynamic-programming backwards pass to fill e_state.
    /// Not available on wasm32 (rayon dependency; call from_e_state with a precomputed table instead).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn solve(&mut self) {

        // Initialize terminal states: all categories filled, upper values 0..63, yahtzee bonus t/f
        (0u8..=63).for_each(|upper_total| {
            [false, true].into_iter().for_each(|yahtzee| {
                let state = state::encode_state(state::FILLED_CATEGORIES, upper_total, yahtzee);
                self.e_state[state] = if upper_total >= 63 { 35.0 } else { 0.0 };
            })
        });

        // Step backwards from 12 categories filled down to 0.
        for n_filled in (0..state::NUM_CATEGORIES).rev() {
            println!("Working back through categories, at stage {n_filled}");

            let stage_states: Vec<(u16, u8, bool)> = (0u16..(1 << state::NUM_CATEGORIES))
                .filter(|c| c.count_ones() == n_filled as u32)
                .flat_map(|categories| {
                    (0u8..=63).flat_map(move |upper_total| {
                        [false, true].map(move |yahtzee| (categories, upper_total, yahtzee))
                    })
                })
                .collect();

            let updates: Vec<(usize, f64)> = {
                let solver = &*self;
                stage_states.par_iter()
                    .map(|&(categories, upper_total, yahtzee)| {
                        let state = state::encode_state(categories, upper_total, yahtzee);
                        (state, solver.compute_widget_e_value(categories, upper_total, yahtzee))
                    })
                    .collect()
            };

            for (state, value) in updates {
                self.e_state[state] = value;
            }
        }
    }

    /// Given a game state and current dice roll, return the optimal action:
    /// - 0 rolls remaining → which category to fill (FillCategory)
    /// - 1-2 rolls remaining → which dice to keep (KeepDice)
    pub fn advise(
        &self,
        categories: u16,
        upper_total: u8,
        yahtzee_bonus: bool,
        roll: &rolls::Roll,
        rolls_remaining: u8,
    ) -> Advice {
        let (advice, _ev) = self.advise_with_ev(categories, upper_total, yahtzee_bonus, roll, rolls_remaining);
        advice
    }

    /// Like advise, but also returns the expected value of the recommended action.
    pub fn advise_with_ev(
        &self,
        categories: u16,
        upper_total: u8,
        yahtzee_bonus: bool,
        roll: &rolls::Roll,
        rolls_remaining: u8,
    ) -> (Advice, f64) {
        let roll_idx = self.all_rolls.iter().position(|r| r == roll)
            .expect("roll not found — dice values must be 1-6");

        match rolls_remaining {
            0 => {
                let scores = &self.score_table[roll_idx][yahtzee_bonus as usize];
                let mut best_val = f64::NEG_INFINITY;
                let mut best_cat = 0u8;
                for c in 0..NUM_CATEGORIES {
                    if categories & (1 << c) == 0 {
                        let sc = scores[c as usize] as f64;
                        let ns = state::next_state(categories, upper_total, yahtzee_bonus, roll, c);
                        let val = sc + self.e_state[ns];
                        if val > best_val {
                            best_val = val;
                            best_cat = c;
                        }
                    }
                }
                (Advice::FillCategory(best_cat), best_val)
            }
            1 => {
                let expected_after_last: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
                    self.best_category_value(categories, upper_total, yahtzee_bonus, i)
                );
                let (keep, ev) = self.find_best_keep_with_ev(roll_idx, &expected_after_last);
                (Advice::KeepDice(keep), ev)
            }
            2 => {
                let expected_after_last: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
                    self.best_category_value(categories, upper_total, yahtzee_bonus, i)
                );
                let expected_one_remaining: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
                    self.best_keep_expected(i, &expected_after_last)
                );
                let (keep, ev) = self.find_best_keep_with_ev(roll_idx, &expected_one_remaining);
                (Advice::KeepDice(keep), ev)
            }
            _ => panic!("rolls_remaining must be 0, 1, or 2"),
        }
    }

    /// Expected value of filling a specific category given the current state and roll.
    pub fn ev_of_category(
        &self,
        categories: u16,
        upper_total: u8,
        yahtzee_bonus: bool,
        roll: &rolls::Roll,
        cat: u8,
    ) -> f64 {
        let roll_idx = self.all_rolls.iter().position(|r| r == roll)
            .expect("roll not found — dice values must be 1-6");
        let sc = self.score_table[roll_idx][yahtzee_bonus as usize][cat as usize] as f64;
        let ns = state::next_state(categories, upper_total, yahtzee_bonus, roll, cat);
        sc + self.e_state[ns]
    }

    /// Expected value of keeping a specific dice subset, given rolls_remaining (1 or 2).
    pub fn ev_of_keep(
        &self,
        categories: u16,
        upper_total: u8,
        yahtzee_bonus: bool,
        keep: &rolls::Roll,
        rolls_remaining: u8,
    ) -> f64 {
        let expected_after_last: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_category_value(categories, upper_total, yahtzee_bonus, i)
        );

        let e_next: &[f64] = match rolls_remaining {
            1 => &expected_after_last,
            2 => {
                let buf: Box<[f64; rolls::NUM_ROLLS]> = Box::new(
                    std::array::from_fn(|i| self.best_keep_expected(i, &expected_after_last))
                );
                return self.keep_ev_from_table(keep, &*buf);
            }
            _ => panic!("rolls_remaining must be 1 or 2 for a keep decision"),
        };

        self.keep_ev_from_table(keep, e_next)
    }

    /// Compute EV of a keep given a precomputed e_next table.
    fn keep_ev_from_table(&self, keep: &rolls::Roll, e_next: &[f64]) -> f64 {
        self.all_rolls.iter().enumerate()
            .map(|(t_idx, t)| {
                let p = rolls::transition_prob(keep, t);
                p * e_next[t_idx]
            })
            .sum()
    }

    /// Find the best keep for a given roll and return the kept Roll (not just its value).
    fn find_best_keep(&self, roll_idx: usize, e_next: &[f64]) -> rolls::Roll {
        let (keep, _ev) = self.find_best_keep_with_ev(roll_idx, e_next);
        keep
    }

    fn find_best_keep_with_ev(&self, roll_idx: usize, e_next: &[f64]) -> (rolls::Roll, f64) {
        let roll = &self.all_rolls[roll_idx];
        let keeps = rolls::enumerate_keeps(roll);
        keeps.into_iter()
            .zip(self.precomputed_keep_trans[roll_idx].iter())
            .map(|(keep_roll, transitions)| {
                let val: f64 = transitions.iter()
                    .map(|&(target_idx, prob)| prob * e_next[target_idx])
                    .sum();
                (keep_roll, val)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
    }

    fn compute_widget_e_value(&self, categories: u16, upper_total: u8, yahtzee_bonus: bool) -> f64 {

        // Step A: For each possible final roll r, compute E(S, r, 0)
        let expected_after_last_roll: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_category_value(categories, upper_total, yahtzee_bonus, i)
        );

        // Step B/C: E(S, r, 1) = max over r' ⊆ r of E over reroll outcomes
        let expected_one_roll_remaining: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_keep_expected(i, &expected_after_last_roll)
        );

        // Step D/E: Same one level up
        let expected_two_rolls_remaining: [f64; rolls::NUM_ROLLS] = std::array::from_fn(|i|
            self.best_keep_expected(i, &expected_one_roll_remaining)
        );

        // Step F: E(S) = Σ_r P(∅ → r) * E(S, r, 2)
        self.trans_from_empty
            .iter()
            .zip(expected_two_rolls_remaining.iter())
            .map(|(p, e)| p * e)
            .sum()
    }

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
        best
    }

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
