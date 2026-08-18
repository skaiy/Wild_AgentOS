fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile(&["proto/se_app.proto"], &["proto"])?;

    println!("cargo:rerun-if-changed=proto/se_app.proto");

    Ok(())
}