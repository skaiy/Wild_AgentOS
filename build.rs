use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);

    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile(
            &["proto/pdca_core.proto"],
            &["proto"],
        )?;

    Ok(())
}
