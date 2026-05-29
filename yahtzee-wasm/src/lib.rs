use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use yahtzee_core::rolls::Roll;
use yahtzee_solver::{Advice, Solver};

// ============================================================================
// Embedded precomputed EV table (little-endian f64, 1M entries = 8MB)
// ============================================================================

static E_STATE_BYTES: &[u8] = include_bytes!("../../e_state.bin");

static SOLVER: OnceLock<Solver> = OnceLock::new();

fn get_solver() -> &'static Solver {
    SOLVER.get_or_init(|| {
        let e_state: Vec<f64> = E_STATE_BYTES
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        Solver::from_e_state(e_state)
    })
}

// ============================================================================
// Category name mapping (TS camelCase ↔ solver index 0–12)
// ============================================================================

fn name_to_idx(name: &str) -> Option<u8> {
    match name {
        "ones" => Some(0),
        "twos" => Some(1),
        "threes" => Some(2),
        "fours" => Some(3),
        "fives" => Some(4),
        "sixes" => Some(5),
        "threeOfAKind" => Some(6),
        "fourOfAKind" => Some(7),
        "fullHouse" => Some(8),
        "smallStraight" => Some(9),
        "largeStraight" => Some(10),
        "yahtzee" => Some(11),
        "chance" => Some(12),
        _ => None,
    }
}

fn idx_to_name(idx: u8) -> &'static str {
    match idx {
        0 => "ones",
        1 => "twos",
        2 => "threes",
        3 => "fours",
        4 => "fives",
        5 => "sixes",
        6 => "threeOfAKind",
        7 => "fourOfAKind",
        8 => "fullHouse",
        9 => "smallStraight",
        10 => "largeStraight",
        11 => "yahtzee",
        12 => "chance",
        _ => panic!("invalid category index"),
    }
}

// ============================================================================
// State parsing helpers
// ============================================================================

fn parse_categories(cats: &HashMap<String, Option<u32>>) -> (u16, u8, bool) {
    let mut bitmask: u16 = 0;
    let mut upper_sum: u32 = 0;
    let mut yahtzee_bonus = false;

    for (name, &score) in cats {
        let Some(idx) = name_to_idx(name) else { continue };
        if score.is_some() {
            bitmask |= 1 << idx;
        }
        if idx <= 5 {
            upper_sum += score.unwrap_or(0);
        }
        if idx == 11 && score == Some(50) {
            yahtzee_bonus = true;
        }
    }

    (bitmask, upper_sum.min(63) as u8, yahtzee_bonus)
}

fn dice_to_roll(dice: &[u8; 5]) -> Roll {
    let mut roll = [0u8; 6];
    for &d in dice {
        roll[d as usize - 1] += 1;
    }
    roll
}

/// Convert a keep Roll (multiset) back to a position-based boolean array.
/// Greedily assigns kept faces to the earliest positions in `dice`.
fn roll_to_bools(dice: &[u8; 5], keep_roll: &Roll) -> [bool; 5] {
    let mut remaining = *keep_roll;
    let mut keep = [false; 5];
    for (i, &d) in dice.iter().enumerate() {
        let face = d as usize - 1;
        if remaining[face] > 0 {
            keep[i] = true;
            remaining[face] -= 1;
        }
    }
    keep
}

/// Convert a position-based boolean keep back to a Roll (multiset).
fn bools_to_roll(dice: &[u8; 5], keep: &[bool; 5]) -> Roll {
    let mut roll = [0u8; 6];
    for (&d, &k) in dice.iter().zip(keep.iter()) {
        if k {
            roll[d as usize - 1] += 1;
        }
    }
    roll
}

// ============================================================================
// JSON types matching the TS contract
// ============================================================================

#[derive(Deserialize)]
struct RecommendRequest {
    categories: HashMap<String, Option<u32>>,
    dice: [u8; 5],
    rolls_remaining: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action")]
enum Response {
    #[serde(rename = "fillCategory")]
    FillCategory {
        category: String,
        expected_score: f64,
    },
    #[serde(rename = "keepDice")]
    KeepDice {
        keep: [bool; 5],
        expected_score: f64,
    },
}

/// User's action for rating — same shape as Response but expected_score is optional
#[derive(Deserialize)]
#[serde(tag = "action")]
enum UserChoice {
    #[serde(rename = "fillCategory")]
    FillCategory { category: String },
    #[serde(rename = "keepDice")]
    KeepDice { keep: [bool; 5] },
}

#[derive(Serialize)]
struct RatingResult {
    rating: &'static str,
    delta: f64,
    user_ev: f64,
    rec_ev: f64,
}

// ============================================================================
// WASM-exported functions
// ============================================================================

/// Given the current game state, return the optimal action and its expected score.
///
/// Input: JSON `RecommendRequest`
/// Output: JSON `RecommendResponse` (FillCategoryResponse | KeepDiceResponse)
#[wasm_bindgen]
pub fn recommend(request_json: &str) -> String {
    let req: RecommendRequest = serde_json::from_str(request_json)
        .expect("invalid RecommendRequest JSON");

    let solver = get_solver();
    let (bitmask, upper_total, yahtzee_bonus) = parse_categories(&req.categories);
    let roll = dice_to_roll(&req.dice);

    let (advice, ev) = solver.advise_with_ev(bitmask, upper_total, yahtzee_bonus, &roll, req.rolls_remaining);

    let response = match advice {
        Advice::FillCategory(cat) => Response::FillCategory {
            category: idx_to_name(cat).to_string(),
            expected_score: ev,
        },
        Advice::KeepDice(keep_roll) => Response::KeepDice {
            keep: roll_to_bools(&req.dice, &keep_roll),
            expected_score: ev,
        },
    };

    serde_json::to_string(&response).unwrap()
}

/// Rate a player's decision against the optimal recommendation.
///
/// Inputs (all JSON):
///   - `state_json`: `RecommendRequest` — the game state when the decision was made
///   - `user_choice_json`: `{ action: 'fillCategory', category: string } | { action: 'keepDice', keep: bool[5] }`
///   - `rec_json`: the `RecommendResponse` previously returned by `recommend`
///
/// Output: JSON `RatingResult`
#[wasm_bindgen]
pub fn rate_decision(state_json: &str, user_choice_json: &str, rec_json: &str) -> String {
    let state: RecommendRequest = serde_json::from_str(state_json)
        .expect("invalid state JSON");
    let user_choice: UserChoice = serde_json::from_str(user_choice_json)
        .expect("invalid user_choice JSON");
    let rec: Response = serde_json::from_str(rec_json)
        .expect("invalid rec JSON");

    let solver = get_solver();
    let (bitmask, upper_total, yahtzee_bonus) = parse_categories(&state.categories);
    let roll = dice_to_roll(&state.dice);

    let user_ev = match &user_choice {
        UserChoice::FillCategory { category } => {
            let cat = name_to_idx(category).expect("unknown category name");
            solver.ev_of_category(bitmask, upper_total, yahtzee_bonus, &roll, cat)
        }
        UserChoice::KeepDice { keep } => {
            let keep_roll = bools_to_roll(&state.dice, keep);
            solver.ev_of_keep(bitmask, upper_total, yahtzee_bonus, &keep_roll, state.rolls_remaining)
        }
    };

    let rec_ev = match &rec {
        Response::FillCategory { expected_score, .. } => *expected_score,
        Response::KeepDice { expected_score, .. } => *expected_score,
    };

    let delta = user_ev - rec_ev;
    let rating = if delta >= 0.0 {
        "optimal"
    } else if delta >= -2.5 {
        "okay"
    } else {
        "mistake"
    };

    serde_json::to_string(&RatingResult { rating, delta, user_ev, rec_ev }).unwrap()
}
