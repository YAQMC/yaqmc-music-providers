use std::{env, fs, path::PathBuf, process::ExitCode};

use wit_component::ComponentEncoder;

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let input = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "missing input Wasm path".to_owned())?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "missing output component path".to_owned())?;
    if arguments.next().is_some() {
        return Err("unexpected extra arguments".to_owned());
    }

    let module = fs::read(&input).map_err(|error| format!("could not read input Wasm: {error}"))?;
    let component = ComponentEncoder::default()
        .module(&module)
        .map_err(|error| format!("could not register input Wasm: {error:#}"))?
        .validate(true)
        .encode()
        .map_err(|error| format!("could not encode component: {error:#}"))?;
    fs::write(&output, component).map_err(|error| format!("could not write component: {error}"))?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
