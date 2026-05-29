use std::path::Path;
use std::sync::Arc;

use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use yahtzee_core::persistence;
use yahtzee_solver::{Advice, Solver};

// ============================================================================
// Request / response types
// ============================================================================

/// Scores for each category. `None` means the category is not yet filled.
/// For Yahtzee: `Some(50)` = filled and bonus-eligible, `Some(0)` = scratched (no bonus).
#[derive(Deserialize)]
struct CategoryScores {
    ones: Option<u32>,
    twos: Option<u32>,
    threes: Option<u32>,
    fours: Option<u32>,
    fives: Option<u32>,
    sixes: Option<u32>,
    three_of_a_kind: Option<u32>,
    four_of_a_kind: Option<u32>,
    full_house: Option<u32>,
    small_straight: Option<u32>,
    large_straight: Option<u32>,
    /// None = unfilled, Some(50) = scored (future bonuses enabled), Some(0) = scratched
    yahtzee: Option<u32>,
    chance: Option<u32>,
}

#[derive(Deserialize)]
struct AdviceRequest {
    categories: CategoryScores,
    /// The five dice showing, e.g. [2, 3, 1, 1, 2]. Values must be 1–6.
    dice: [u8; 5],
    /// How many rolls are left this turn: 0 (must fill), 1, or 2.
    rolls_remaining: u8,
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AdviceResponse {
    /// No rolls left — fill this category.
    FillCategory { category: String },
    /// Rolls remaining — keep these dice and reroll the rest.
    KeepDice { keep: Vec<u8> },
}

// ============================================================================
// Translation from API types to solver types
// ============================================================================

fn to_categories_bitmask(c: &CategoryScores) -> u16 {
    let filled = [
        c.ones, c.twos, c.threes, c.fours, c.fives, c.sixes,
        c.three_of_a_kind, c.four_of_a_kind, c.full_house,
        c.small_straight, c.large_straight, c.yahtzee, c.chance,
    ];
    filled.iter().enumerate().fold(0u16, |mask, (i, score)| {
        if score.is_some() { mask | (1 << i) } else { mask }
    })
}

fn to_upper_total(c: &CategoryScores) -> u8 {
    let sum = c.ones.unwrap_or(0)
        + c.twos.unwrap_or(0)
        + c.threes.unwrap_or(0)
        + c.fours.unwrap_or(0)
        + c.fives.unwrap_or(0)
        + c.sixes.unwrap_or(0);
    sum.min(63) as u8
}

/// Yahtzee bonus is enabled when the Yahtzee category was scored with 50 (not scratched).
fn to_yahtzee_bonus(c: &CategoryScores) -> bool {
    c.yahtzee == Some(50)
}

/// Convert [d1, d2, d3, d4, d5] face values into a Roll (counts per face).
fn dice_to_roll(dice: &[u8; 5]) -> yahtzee_core::rolls::Roll {
    let mut roll = [0u8; 6];
    for &d in dice {
        roll[d as usize - 1] += 1;
    }
    roll
}

/// Convert a Roll (counts per face) back to a sorted list of face values.
fn roll_to_dice(roll: &yahtzee_core::rolls::Roll) -> Vec<u8> {
    let mut dice = Vec::new();
    for (face_idx, &count) in roll.iter().enumerate() {
        for _ in 0..count {
            dice.push(face_idx as u8 + 1);
        }
    }
    dice
}

fn category_name(cat: u8) -> &'static str {
    match cat {
        0 => "ones",
        1 => "twos",
        2 => "threes",
        3 => "fours",
        4 => "fives",
        5 => "sixes",
        6 => "three_of_a_kind",
        7 => "four_of_a_kind",
        8 => "full_house",
        9 => "small_straight",
        10 => "large_straight",
        11 => "yahtzee",
        12 => "chance",
        _ => panic!("invalid category index"),
    }
}

// ============================================================================
// Route handler
// ============================================================================

async fn advice(
    State(solver): State<Arc<Solver>>,
    Json(req): Json<AdviceRequest>,
) -> Json<AdviceResponse> {
    let categories = to_categories_bitmask(&req.categories);
    let upper_total = to_upper_total(&req.categories);
    let yahtzee_bonus = to_yahtzee_bonus(&req.categories);
    let roll = dice_to_roll(&req.dice);

    let result = solver.advise(categories, upper_total, yahtzee_bonus, &roll, req.rolls_remaining);

    let response = match result {
        Advice::FillCategory(cat) => AdviceResponse::FillCategory {
            category: category_name(cat).to_string(),
        },
        Advice::KeepDice(keep) => AdviceResponse::KeepDice {
            keep: roll_to_dice(&keep),
        },
    };

    Json(response)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    let path = Path::new(persistence::E_STATE_FILE);
    println!("Loading e_state from {:?}...", path);
    let e_state = persistence::load_e_state(path).expect(
        "Failed to load e_state — run the solver first with `cargo run --release --bin solve`",
    );
    println!("Loaded {} entries. Building lookup tables...", e_state.len());

    let solver = Arc::new(Solver::from_e_state(e_state));
    println!("Ready.");

    let app = Router::new()
        .route("/advice", post(advice))
        .with_state(solver);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
