use std::path::Path;
use std::time::Instant;
use yahtzee_core::{persistence, state};
use yahtzee_solver::Solver;

fn main() {
    println!("\nSolving... (this takes a few minutes)");
    let t0 = Instant::now();
    let mut solver = Solver::new();
    solver.solve();
    println!("Solved in {:.1}s", t0.elapsed().as_secs_f64());

    let initial = state::encode_state(0, 0, false);
    println!("\nExpected score under optimal play: {:.4}", solver.e_state[initial]);
    println!("(Glenn reports 254.59 with Yahtzee bonuses; 245.87 without)");

    let path = Path::new(persistence::E_STATE_FILE);
    print!("\nSaving e_state to {:?}... ", path);
    persistence::save_e_state(&solver.e_state, path).expect("failed to save e_state");
    println!("done.");
}
