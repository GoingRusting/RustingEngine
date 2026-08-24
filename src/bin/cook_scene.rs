use std::error::Error;
use std::path::PathBuf;

use rusting_engine::runtime::cook_scene;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let source = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: cook_scene <source.rscene> <output.rscene.bin>")?;
    let destination = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: cook_scene <source.rscene> <output.rscene.bin>")?;
    if arguments.next().is_some() {
        return Err(
            "usage: cook_scene <source.rscene> <output.rscene.bin>".into()
        );
    }
    cook_scene(&source, &destination)?;
    println!("Cooked {} -> {}", source.display(), destination.display());
    Ok(())
}
