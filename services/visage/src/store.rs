//! Empreintes enregistrées : une ligne de nombres par image, dans un fichier lisible par
//! l'utilisateur seul (dossier 0700, fichier 0600).

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
};

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;
    Some(base.join("ferrite/visage/empreintes"))
}

pub fn exists() -> bool {
    path().is_some_and(|path| path.is_file())
}

pub fn save(prints: &[Vec<f32>]) -> Result<(), String> {
    let path = path().ok_or("dossier personnel inconnu")?;
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|err| err.to_string())?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|err| err.to_string())?;
    file.write_all(encode(prints).as_bytes())
        .map_err(|err| err.to_string())
}

pub fn load() -> Result<Vec<Vec<f32>>, String> {
    let Some(path) = path().filter(|path| path.is_file()) else {
        return Ok(Vec::new());
    };
    fs::read_to_string(path)
        .map(|text| decode(&text))
        .map_err(|err| err.to_string())
}

pub fn forget() -> Result<(), String> {
    match path() {
        Some(path) if path.is_file() => fs::remove_file(path).map_err(|err| err.to_string()),
        _ => Ok(()),
    }
}

fn encode(prints: &[Vec<f32>]) -> String {
    prints
        .iter()
        .map(|print| {
            let values: Vec<String> = print.iter().map(f32::to_string).collect();
            values.join(" ") + "\n"
        })
        .collect()
}

fn decode(text: &str) -> Vec<Vec<f32>> {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<f32>>()
        })
        .filter(|print| print.len() == 128)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_des_empreintes() {
        let prints = vec![vec![0.25_f32; 128], vec![-0.5_f32; 128]];
        assert_eq!(decode(&encode(&prints)), prints);
        // Une ligne abîmée est ignorée.
        assert_eq!(decode("1 2 3\n").len(), 0);
    }
}
