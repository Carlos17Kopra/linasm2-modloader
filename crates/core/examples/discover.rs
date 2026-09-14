fn main() {
    match sm2_core::paths::GamePaths::discover() {
        Ok(p) => {
            println!("Spiel:  {}", p.game_dir.display());
            println!("Mods:   {}", p.mods_dir().display());
            match p.save_dir(None) {
                Ok(s) => println!("Saves:  {}", s.display()),
                Err(e) => println!("Saves:  {e}"),
            }
        }
        Err(e) => println!("Fehler: {e}"),
    }
}
