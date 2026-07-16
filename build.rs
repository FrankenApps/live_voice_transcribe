fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=assets/icon/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/icon.ico");
        res.set_language(0x0000); // Neutral language
        res.set("Comments", "Source code is licensed under the MIT License.");
        res.set("CompanyName", "FrankenApps");
        res.set("FileDescription", "Transcribes voice input to text.");
        res.set("InternalName", "live_voice_transcribe");
        res.set("LegalCopyright", "Copyright © 2026 FrankenApps");
        res.set("OriginalFilename", "live_voice_transcribe.exe");
        res.set("ProductName", "Live Voice Transcribe");
        res.set("ProductVersion", "0.1.0");

        res.compile()?;
    }

    Ok(())
}
