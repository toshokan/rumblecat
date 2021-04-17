fn main() -> std::io::Result<()> {
    use prost_build::Config;

    let mut cfg = Config::new();
    cfg.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    cfg.compile_protos(&["src/Mumble.proto"], &["src/"])?;
    Ok(())
}
