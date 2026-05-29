use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use crate::state::TABLE_SIZE;

pub const E_STATE_FILE: &str = "e_state.bin";

pub fn save_e_state(e_state: &[f64], path: &Path) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    for &value in e_state {
        writer.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

pub fn load_e_state(path: &Path) -> std::io::Result<Vec<f64>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = [0u8; 8];
    let mut e_state = Vec::with_capacity(TABLE_SIZE);
    for _ in 0..TABLE_SIZE {
        reader.read_exact(&mut buf)?;
        e_state.push(f64::from_le_bytes(buf));
    }
    Ok(e_state)
}
